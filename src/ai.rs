use std::{fs, path::Path, time::Duration};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};

use crate::{
    cli::ReasoningEffort,
    domain::{FileKind, ImpactAnalysis, RepoFile, RepoModel},
};

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const IMAGE_TOOL_MODEL: &str = "gpt-5.2";
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 2_500;
const DEFAULT_MAX_PROMPT_FILES: usize = 80;
const MAX_SYMBOLS_PER_FILE: usize = 8;
const MAX_IMPORTS_PER_FILE: usize = 8;
const MAX_SNIPPETS_PER_FILE: usize = 2;
const MAX_SNIPPET_CHARS: usize = 140;

#[derive(Clone)]
pub struct OpenAiClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    reasoning_effort: ReasoningEffort,
    max_output_tokens: u32,
    max_prompt_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiStreamEvent {
    Created,
    TextDelta(String),
    Completed,
}

impl OpenAiClient {
    pub fn new(api_key: String, model: String, reasoning_effort: ReasoningEffort) -> Self {
        Self {
            client: build_http_client(),
            api_key,
            base_url: OPENAI_RESPONSES_URL.to_string(),
            model,
            reasoning_effort,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            max_prompt_files: DEFAULT_MAX_PROMPT_FILES,
        }
    }

    pub fn with_base_url(
        api_key: String,
        base_url: String,
        model: String,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        Self {
            client: build_http_client(),
            api_key,
            base_url,
            model,
            reasoning_effort,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            max_prompt_files: DEFAULT_MAX_PROMPT_FILES,
        }
    }

    pub fn with_limits(mut self, max_output_tokens: u32, max_prompt_files: usize) -> Self {
        self.max_output_tokens = max_output_tokens;
        self.max_prompt_files = max_prompt_files;
        self
    }

    pub async fn analyze_streaming<F>(
        &self,
        repo_model: &RepoModel,
        change_request: &str,
        mut on_event: F,
    ) -> Result<ImpactAnalysis>
    where
        F: FnMut(AiStreamEvent),
    {
        let user_prompt = build_user_prompt(repo_model, change_request, self.max_prompt_files)?;
        let first_body = build_request(
            &self.model,
            self.reasoning_effort,
            &system_prompt(),
            &user_prompt,
            true,
            self.max_output_tokens,
        );
        let first_text = self.send_stream(first_body, &mut on_event).await?;
        match parse_impact_analysis(&first_text) {
            Ok(analysis) => Ok(analysis),
            Err(first_error) => {
                let corrective_prompt = format!(
                    "{user_prompt}\n\nPrevious response failed JSON parsing: {first_error}\nReturn corrected strict JSON only."
                );
                let retry_body = build_request(
                    &self.model,
                    self.reasoning_effort,
                    &system_prompt(),
                    &corrective_prompt,
                    true,
                    self.max_output_tokens,
                );
                let retry_text = self.send_stream(retry_body, &mut on_event).await?;
                parse_impact_analysis(&retry_text)
            }
        }
    }

    pub async fn analyze(
        &self,
        repo_model: &RepoModel,
        change_request: &str,
    ) -> Result<ImpactAnalysis> {
        let user_prompt = build_user_prompt(repo_model, change_request, self.max_prompt_files)?;
        let first_body = build_request(
            &self.model,
            self.reasoning_effort,
            &system_prompt(),
            &user_prompt,
            false,
            self.max_output_tokens,
        );
        let first_text = self.send(first_body).await?;
        match parse_impact_analysis(&first_text) {
            Ok(analysis) => Ok(analysis),
            Err(first_error) => {
                let corrective_prompt = format!(
                    "{user_prompt}\n\nPrevious response failed JSON parsing: {first_error}\nReturn corrected strict JSON only."
                );
                let retry_body = build_request(
                    &self.model,
                    self.reasoning_effort,
                    &system_prompt(),
                    &corrective_prompt,
                    false,
                    self.max_output_tokens,
                );
                let retry_text = self.send(retry_body).await?;
                parse_impact_analysis(&retry_text)
            }
        }
    }

    pub async fn generate_architecture_diagram(
        &self,
        analysis: &ImpactAnalysis,
        change_request: &str,
        output_path: &Path,
    ) -> Result<()> {
        let body = json!({
            "model": IMAGE_TOOL_MODEL,
            "input": architecture_image_prompt(analysis, change_request),
            "tools": [{
                "type": "image_generation",
                "size": "1536x1024",
                "quality": "high"
            }],
            "tool_choice": {"type": "image_generation"}
        });
        let response = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("OpenAI image request failed")?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .with_context(|| format!("OpenAI image response was not JSON, status {status}"))?;
        if !status.is_success() {
            return Err(anyhow!("OpenAI image API error {status}: {value}"));
        }
        let image_base64 = extract_image_base64(&value)
            .ok_or_else(|| anyhow!("OpenAI image response did not contain image data"))?;
        let bytes = STANDARD
            .decode(image_base64)
            .context("OpenAI image data was not valid base64")?;
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output_path, bytes)?;
        Ok(())
    }

    async fn send(&self, body: Value) -> Result<String> {
        let response = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("OpenAI request failed")?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .with_context(|| format!("OpenAI response was not JSON, status {status}"))?;
        if !status.is_success() {
            return Err(anyhow!("OpenAI API error {status}: {value}"));
        }
        extract_response_text(&value)
            .ok_or_else(|| anyhow!("OpenAI response did not contain output text"))
    }

    async fn send_stream<F>(&self, body: Value, on_event: &mut F) -> Result<String>
    where
        F: FnMut(AiStreamEvent),
    {
        let response = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("OpenAI streaming request failed")?;
        let status = response.status();
        if !status.is_success() {
            let value: Value = response
                .json()
                .await
                .unwrap_or_else(|_| json!({"error":"OpenAI error body was not JSON"}));
            return Err(anyhow!("OpenAI API error {status}: {value}"));
        }

        let mut stream = response.bytes_stream();
        let mut pending = String::new();
        let mut output = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("OpenAI streaming chunk failed")?;
            pending.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(index) = pending.find("\n\n") {
                let block = pending[..index].to_string();
                pending = pending[index + 2..].to_string();
                handle_sse_block(&block, &mut output, on_event)?;
            }
        }
        if !pending.trim().is_empty() {
            handle_sse_block(&pending, &mut output, on_event)?;
        }

        if output.trim().is_empty() {
            Err(anyhow!("OpenAI stream ended without output text"))
        } else {
            Ok(output)
        }
    }
}

pub fn parse_impact_analysis(text: &str) -> Result<ImpactAnalysis> {
    let json_text = extract_json_object(text).unwrap_or(text);
    serde_json::from_str::<ImpactAnalysis>(json_text).context("invalid impact analysis JSON")
}

fn build_user_prompt(
    repo_model: &RepoModel,
    change_request: &str,
    max_prompt_files: usize,
) -> Result<String> {
    let compact = compact_repo_model(repo_model, max_prompt_files);
    let repo_json = serde_json::to_string(&compact)?;
    Ok(format!(
        "Repository summary, compact and capped:\n{repo_json}\n\nProposed change:\n{change_request}\n\nTask: return strict JSON only. Pick the most likely impacted files. Keep reasons and plans terse."
    ))
}

fn system_prompt() -> String {
    "You are a senior software architect and change impact analyst. Predict likely blast radius from repository summary. Do not invent files that are not present unless clearly marked as proposed new files. Prioritize practical reasoning, implementation tradeoffs, risks, and tests. Return strictly valid JSON matching requested schema.".to_string()
}

fn architecture_image_prompt(analysis: &ImpactAnalysis, change_request: &str) -> String {
    format!(
        "Draw a label-light developer architecture blueprint for Branch Futures.\n\
         Proposed change: {change_request}\n\
         Summary: {}\n\
         Visual structure: left impact zone with 4-6 stacked modules, center trace corridor with branching arrows, right futures zone with 3 option lanes, bottom recommendation rail.\n\
         Style: crisp technical blueprint, terminal-inspired, dark background, cyan and blue glow, fine grid, clear boxes, arrows, zones, and connection lines.\n\
         Text constraints: no file paths, no tiny labels, no dense text, no pseudo text, no gibberish. Use at most these broad zone labels: Impact, Trace, Futures, Recommended.\n\
         Leave generous empty space where terminal overlay labels will be rendered.",
        analysis.summary
    )
}

fn build_request(
    model: &str,
    reasoning_effort: ReasoningEffort,
    system: &str,
    user: &str,
    stream: bool,
    max_output_tokens: u32,
) -> Value {
    let mut body = json!({
        "model": model,
        "input": [
            {"role": "system", "content": [{"type": "input_text", "text": system}]},
            {"role": "user", "content": [{"type": "input_text", "text": user}]}
        ],
        "max_output_tokens": max_output_tokens,
        "text": {
            "format": {
                "type": "json_schema",
                "name": "impact_analysis",
                "strict": true,
                "schema": impact_schema()
            }
        }
    });
    if let Some(effort) = reasoning_effort.as_api_value() {
        body["reasoning"] = json!({ "effort": effort });
    }
    if stream {
        body["stream"] = json!(true);
    }
    body
}

fn compact_repo_model(repo_model: &RepoModel, max_prompt_files: usize) -> RepoModel {
    let mut files = repo_model.files.clone();
    files.sort_by(|a, b| file_priority(a).cmp(&file_priority(b)));
    files.truncate(max_prompt_files);
    for file in &mut files {
        file.symbols.truncate(MAX_SYMBOLS_PER_FILE);
        file.imports.truncate(MAX_IMPORTS_PER_FILE);
        file.snippets.truncate(MAX_SNIPPETS_PER_FILE);
        for snippet in &mut file.snippets {
            if snippet.len() > MAX_SNIPPET_CHARS {
                snippet.truncate(MAX_SNIPPET_CHARS);
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    RepoModel {
        repo_name: repo_model.repo_name.clone(),
        root_path: repo_model.root_path.clone(),
        frameworks: repo_model.frameworks.clone(),
        files,
        routes: repo_model.routes.iter().take(40).cloned().collect(),
        tests: repo_model.tests.iter().take(40).cloned().collect(),
        config_files: repo_model.config_files.iter().take(30).cloned().collect(),
        risk_signals: repo_model.risk_signals.iter().take(30).cloned().collect(),
    }
}

fn file_priority(file: &RepoFile) -> (u8, &str) {
    let kind_priority = match file.kind {
        FileKind::Route => 0,
        FileKind::Schema => 1,
        FileKind::Worker | FileKind::Service | FileKind::Controller => 2,
        FileKind::UiComponent => 3,
        FileKind::Config => 4,
        FileKind::Test => 5,
        FileKind::TypeScript | FileKind::JavaScript | FileKind::Python | FileKind::Rust => 6,
        FileKind::Unknown => 9,
    };
    (kind_priority, file.path.as_str())
}

fn build_http_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn impact_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "impact_path", "risk_summary", "tests_to_add", "futures", "recommended_future"],
        "properties": {
            "summary": {"type": "string"},
            "impact_path": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["path", "reason", "impact_score", "confidence", "risk", "change_needed"],
                    "properties": {
                        "path": {"type": "string"},
                        "reason": {"type": "string"},
                        "impact_score": {"type": "integer", "minimum": 0, "maximum": 100},
                        "confidence": {"type": "integer", "minimum": 0, "maximum": 100},
                        "risk": {"type": "string", "enum": ["low", "medium", "high"]},
                        "change_needed": {"type": "string"}
                    }
                }
            },
            "risk_summary": {"type": "array", "items": {"type": "string"}},
            "tests_to_add": {"type": "array", "items": {"type": "string"}},
            "futures": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "description", "complexity", "risk", "affected_files", "benefits", "drawbacks", "patch_plan", "test_plan"],
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "complexity": {"type": "string", "enum": ["low", "medium", "high"]},
                        "risk": {"type": "string", "enum": ["low", "medium", "high"]},
                        "affected_files": {"type": "array", "items": {"type": "string"}},
                        "benefits": {"type": "array", "items": {"type": "string"}},
                        "drawbacks": {"type": "array", "items": {"type": "string"}},
                        "patch_plan": {"type": "array", "items": {"type": "string"}},
                        "test_plan": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "recommended_future": {"type": "string"}
        }
    })
}

fn extract_response_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    find_text(value)
}

fn extract_image_base64(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => {
            if matches!(
                map.get("type").and_then(Value::as_str),
                Some("image_generation_call")
            ) {
                if let Some(result) = map.get("result").and_then(Value::as_str) {
                    return Some(result);
                }
            }
            if let Some(image) = map.get("b64_json").and_then(Value::as_str) {
                return Some(image);
            }
            for child in map.values() {
                if let Some(image) = extract_image_base64(child) {
                    return Some(image);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_image_base64),
        _ => None,
    }
}

fn find_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if matches!(
                map.get("type").and_then(Value::as_str),
                Some("output_text" | "text")
            ) {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
            }
            for child in map.values() {
                if let Some(text) = find_text(child) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_text),
        Value::String(text) if text.trim_start().starts_with('{') => Some(text.clone()),
        _ => None,
    }
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

fn handle_sse_block<F>(block: &str, output: &mut String, on_event: &mut F) -> Result<()>
where
    F: FnMut(AiStreamEvent),
{
    let mut data = String::new();
    for line in block.lines() {
        let line = line.trim_end();
        if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim_start());
        }
    }
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }

    let value: Value = serde_json::from_str(&data).context("invalid OpenAI stream event JSON")?;
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "response.created" => on_event(AiStreamEvent::Created),
        "response.output_text.delta" => {
            let delta = value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            output.push_str(&delta);
            on_event(AiStreamEvent::TextDelta(delta));
        }
        "response.output_text.done" => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                output.clear();
                output.push_str(text);
            }
        }
        "response.completed" => on_event(AiStreamEvent::Completed),
        "error" => return Err(anyhow!("OpenAI stream error: {value}")),
        _ => {}
    }
    Ok(())
}
