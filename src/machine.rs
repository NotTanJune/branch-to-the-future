use std::{env, io};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    ai::OpenAiClient,
    cli::{dotenv_search_dirs_for, validate_repo_input, AnalyzeCommand, ScanCommand},
    domain::{ImpactAnalysis, RepoModel},
    repo::scan_repo,
    repo_source::prepare_repo_source,
};

const SCHEMA_VERSION: u8 = 1;
const TOOL_NAME: &str = "branch-to-the-future";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonEnvelope {
    pub schema_version: u8,
    pub tool: String,
    pub created_at: String,
    pub change_request: String,
    pub repo_model: RepoModel,
    pub impact_analysis: Option<ImpactAnalysis>,
    pub limits: JsonLimits,
    pub model: JsonModel,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonLimits {
    pub max_file_bytes: u64,
    pub ignore: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_prompt_files: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonModel {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_output_tokens: Option<u32>,
}

impl JsonLimits {
    pub fn new(max_file_bytes: u64, ignore: Vec<String>, max_prompt_files: Option<usize>) -> Self {
        Self {
            max_file_bytes,
            ignore,
            max_prompt_files,
        }
    }
}

impl JsonModel {
    pub fn openai(name: String, reasoning_effort: String, max_output_tokens: u32) -> Self {
        Self {
            provider: Some("openai".to_string()),
            name: Some(name),
            reasoning_effort: Some(reasoning_effort),
            max_output_tokens: Some(max_output_tokens),
        }
    }
}

pub fn build_scan_envelope(
    repo_model: RepoModel,
    limits: JsonLimits,
    created_at: String,
) -> JsonEnvelope {
    JsonEnvelope {
        schema_version: SCHEMA_VERSION,
        tool: TOOL_NAME.to_string(),
        created_at,
        change_request: String::new(),
        repo_model,
        impact_analysis: None,
        limits,
        model: JsonModel::default(),
        warnings: Vec::new(),
    }
}

pub fn build_analyze_envelope(
    repo_model: RepoModel,
    impact_analysis: ImpactAnalysis,
    change_request: String,
    limits: JsonLimits,
    model: JsonModel,
    created_at: String,
) -> JsonEnvelope {
    JsonEnvelope {
        schema_version: SCHEMA_VERSION,
        tool: TOOL_NAME.to_string(),
        created_at,
        change_request,
        repo_model,
        impact_analysis: Some(impact_analysis),
        limits,
        model,
        warnings: Vec::new(),
    }
}

pub fn run_scan_json(command: &ScanCommand) -> Result<JsonEnvelope> {
    if !command.json {
        bail!("scan currently supports only --json");
    }
    validate_repo_input(&command.repo_path)?;
    let search_dirs = dotenv_search_dirs_for(&command.repo_path)?;
    let prepared_repo = prepare_repo_source(&command.repo_path, &search_dirs)?;
    let repo_model = scan_repo(
        &prepared_repo.local_path,
        command.max_file_bytes,
        &command.ignore,
    )?;
    Ok(build_scan_envelope(
        repo_model,
        JsonLimits::new(command.max_file_bytes, command.ignore.clone(), None),
        created_at_rfc3339()?,
    ))
}

pub async fn run_analyze_json(command: &AnalyzeCommand) -> Result<JsonEnvelope> {
    if !command.json {
        bail!("analyze currently supports only --json");
    }
    validate_repo_input(&command.repo_path)?;
    let search_dirs = dotenv_search_dirs_for(&command.repo_path)?;
    let api_key = crate::cli::resolve_openai_api_key(
        env::var("OPENAI_API_KEY").ok().as_deref(),
        &search_dirs,
    )?;
    if api_key.trim().is_empty() {
        bail!("OPENAI_API_KEY is required before analysis");
    }

    let prepared_repo = prepare_repo_source(&command.repo_path, &search_dirs)?;
    let repo_model = scan_repo(
        &prepared_repo.local_path,
        command.max_file_bytes,
        &command.ignore,
    )?;
    let model_name = command.resolved_model();
    let client = openai_client(command, api_key, model_name.clone());
    let impact_analysis = client.analyze(&repo_model, &command.change).await?;
    Ok(build_analyze_envelope(
        repo_model,
        impact_analysis,
        command.change.clone(),
        JsonLimits::new(
            command.max_file_bytes,
            command.ignore.clone(),
            Some(command.max_prompt_files),
        ),
        JsonModel::openai(
            model_name,
            command
                .reasoning_effort
                .as_api_value()
                .unwrap_or("none")
                .to_string(),
            command.max_output_tokens,
        ),
        created_at_rfc3339()?,
    ))
}

pub fn write_json(envelope: &JsonEnvelope) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, envelope)?;
    use std::io::Write;
    writeln!(&mut handle)?;
    Ok(())
}

fn openai_client(command: &AnalyzeCommand, api_key: String, model_name: String) -> OpenAiClient {
    let base_url = env::var("BRANCH_TO_THE_FUTURE_OPENAI_BASE_URL").ok();
    let client = match base_url {
        Some(base_url) if !base_url.trim().is_empty() => {
            OpenAiClient::with_base_url(api_key, base_url, model_name, command.reasoning_effort)
        }
        _ => OpenAiClient::new(api_key, model_name, command.reasoning_effort),
    };
    client.with_limits(command.max_output_tokens, command.max_prompt_files)
}

pub fn created_at_rfc3339() -> Result<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}
