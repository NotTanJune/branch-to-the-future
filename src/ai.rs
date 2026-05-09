use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Map, Value};

use crate::{
    cli::ReasoningEffort,
    domain::{FileKind, ImpactAnalysis, RepoFile, RepoModel},
};

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8_000;
const MAX_RETRY_OUTPUT_TOKENS: u32 = 16_000;
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
                let corrective_prompt = corrective_prompt(&user_prompt, &first_error);
                let retry_body = build_request(
                    &self.model,
                    self.reasoning_effort,
                    &system_prompt(),
                    &corrective_prompt,
                    true,
                    retry_output_tokens(self.max_output_tokens),
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
                let corrective_prompt = corrective_prompt(&user_prompt, &first_error);
                let retry_body = build_request(
                    &self.model,
                    self.reasoning_effort,
                    &system_prompt(),
                    &corrective_prompt,
                    false,
                    retry_output_tokens(self.max_output_tokens),
                );
                let retry_text = self.send(retry_body).await?;
                parse_impact_analysis(&retry_text)
            }
        }
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
    let mut value =
        serde_json::from_str::<Value>(json_text).context("invalid impact analysis JSON syntax")?;
    normalize_impact_analysis(&mut value);
    serde_json::from_value::<ImpactAnalysis>(value).context("invalid impact analysis JSON")
}

fn normalize_impact_analysis(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };

    take_alias(map, "current_architecture", &["currentArchitecture"]);
    take_alias(
        map,
        "impact_path",
        &["impactPath", "impactFiles", "impact_files"],
    );
    take_alias(map, "risk_summary", &["riskSummary", "risks"]);
    take_alias(
        map,
        "tests_to_add",
        &["testsToAdd", "testPlan", "test_plan"],
    );
    take_alias(
        map,
        "recommended_future",
        &["recommendedFuture", "recommendedPath", "recommended_path"],
    );

    ensure_string(map, "summary", "Impact analysis");
    normalize_stage_array_field(map, "current_architecture");
    normalize_array_field(map, "impact_path");
    if let Some(Value::Array(files)) = map.get_mut("impact_path") {
        for file in files {
            normalize_impact_file(file);
        }
    }
    normalize_string_array_field(map, "risk_summary");
    normalize_string_array_field(map, "tests_to_add");
    normalize_array_field(map, "futures");
    if let Some(Value::Array(futures)) = map.get_mut("futures") {
        for future in futures {
            normalize_future(future);
        }
    }
    ensure_recommended_future(map);
}

fn normalize_impact_file(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };

    take_alias(map, "path", &["file", "filename"]);
    take_alias(map, "impact_score", &["impactScore", "score"]);
    take_alias(map, "change_needed", &["changeNeeded", "change"]);
    lower_string_field(map, "risk");
}

fn normalize_future(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };

    lower_string_field(map, "complexity");
    lower_string_field(map, "risk");
    take_alias(map, "affected_files", &["affectedFiles", "files"]);
    take_alias(map, "patch_plan", &["patchPlan"]);
    take_alias(map, "test_plan", &["testPlan"]);
    normalize_stage_array_field(map, "architecture");
    normalize_string_array_field(map, "affected_files");
    normalize_string_array_field(map, "benefits");
    normalize_string_array_field(map, "drawbacks");
    normalize_string_array_field(map, "patch_plan");
    normalize_string_array_field(map, "test_plan");
}

fn normalize_stage_array_field(map: &mut Map<String, Value>, key: &str) {
    normalize_array_field(map, key);
    if let Some(Value::Array(stages)) = map.get_mut(key) {
        for stage in stages {
            normalize_architecture_stage(stage);
        }
    }
}

fn normalize_architecture_stage(value: &mut Value) {
    match value {
        Value::Object(map) => {
            take_alias(map, "label", &["name", "title", "stage"]);
            ensure_string(map, "label", "Architecture stage");
            normalize_string_array_field(map, "responsibilities");
            normalize_string_array_field(map, "files");
        }
        Value::String(label) => {
            *value = json!({
                "label": label,
                "responsibilities": [],
                "files": []
            });
        }
        _ => {}
    }
}

fn take_alias(map: &mut Map<String, Value>, canonical: &str, aliases: &[&str]) {
    if map.contains_key(canonical) {
        return;
    }
    for alias in aliases {
        if let Some(value) = map.remove(*alias) {
            map.insert(canonical.to_string(), value);
            return;
        }
    }
}

fn normalize_array_field(map: &mut Map<String, Value>, key: &str) {
    let value = map.remove(key).unwrap_or(Value::Array(Vec::new()));
    let normalized = match value {
        Value::Array(items) => Value::Array(items),
        Value::Null => Value::Array(Vec::new()),
        other => Value::Array(vec![other]),
    };
    map.insert(key.to_string(), normalized);
}

fn normalize_string_array_field(map: &mut Map<String, Value>, key: &str) {
    let value = map.remove(key).unwrap_or(Value::Array(Vec::new()));
    let items = match value {
        Value::Array(items) => items,
        Value::Null => Vec::new(),
        other => vec![other],
    }
    .into_iter()
    .filter_map(|item| value_to_string(&item))
    .map(Value::String)
    .collect::<Vec<_>>();
    map.insert(key.to_string(), Value::Array(items));
}

fn ensure_string(map: &mut Map<String, Value>, key: &str, fallback: &str) {
    let value = map
        .remove(key)
        .unwrap_or_else(|| Value::String(fallback.to_string()));
    let normalized = value_to_string(&value)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string());
    map.insert(key.to_string(), Value::String(normalized));
}

fn lower_string_field(map: &mut Map<String, Value>, key: &str) {
    if let Some(Value::String(value)) = map.get_mut(key) {
        *value = value.trim().to_ascii_lowercase();
    }
}

fn ensure_recommended_future(map: &mut Map<String, Value>) {
    if map
        .get("recommended_future")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }
    let fallback = map
        .get("futures")
        .and_then(Value::as_array)
        .and_then(|futures| futures.first())
        .and_then(|future| future.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("No recommendation returned")
        .to_string();
    map.insert("recommended_future".to_string(), Value::String(fallback));
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn corrective_prompt(user_prompt: &str, first_error: &anyhow::Error) -> String {
    format!(
        "{user_prompt}\n\nPrevious response failed JSON parsing: {first_error:#}\nReturn corrected strict JSON only. Keep output compact: at most 5 impact files, 3 futures, 5 architecture stages per architecture, 3 responsibilities per stage, and 5 files per stage."
    )
}

fn retry_output_tokens(current: u32) -> u32 {
    current
        .saturating_mul(2)
        .max(DEFAULT_MAX_OUTPUT_TOKENS)
        .min(MAX_RETRY_OUTPUT_TOKENS)
}

fn build_user_prompt(
    repo_model: &RepoModel,
    change_request: &str,
    max_prompt_files: usize,
) -> Result<String> {
    let compact = compact_repo_model(repo_model, max_prompt_files);
    let repo_json = serde_json::to_string(&compact)?;
    Ok(format!(
        "Repository summary, compact and capped:\n{repo_json}\n\nProposed change:\n{change_request}\n\nTask: return strict JSON only. Pick the most likely impacted files. Keep reasons and plans terse. Generate current_architecture and each future architecture from this repository and change, using concrete repo-specific stage labels, responsibilities, and files. Do not use generic web-only architecture buckets unless the repo actually has those layers. Keep output compact: at most 5 impact files, 3 futures, 5 architecture stages per architecture, 3 responsibilities per stage, and 5 files per stage."
    ))
}

fn system_prompt() -> String {
    "You are a senior software architect and change impact analyst. Predict likely blast radius from repository summary. Generate architecture stages from the actual repo shape and proposed future, for any language or framework. Do not invent files that are not present unless clearly marked as proposed new files. Prioritize practical reasoning, implementation tradeoffs, risks, and tests. Return strictly valid JSON matching requested schema.".to_string()
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
    let stage_schema = architecture_stage_schema();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "current_architecture", "impact_path", "risk_summary", "tests_to_add", "futures", "recommended_future"],
        "properties": {
            "summary": {"type": "string"},
            "current_architecture": {
                "type": "array",
                "items": stage_schema.clone()
            },
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
                    "required": ["name", "description", "complexity", "risk", "architecture", "affected_files", "benefits", "drawbacks", "patch_plan", "test_plan"],
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "complexity": {"type": "string", "enum": ["low", "medium", "high"]},
                        "risk": {"type": "string", "enum": ["low", "medium", "high"]},
                        "architecture": {
                            "type": "array",
                            "items": stage_schema
                        },
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

fn architecture_stage_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["label", "responsibilities", "files"],
        "properties": {
            "label": {"type": "string"},
            "responsibilities": {"type": "array", "items": {"type": "string"}},
            "files": {"type": "array", "items": {"type": "string"}}
        }
    })
}

fn extract_response_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    find_text(value)
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
