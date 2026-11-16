use std::{
    env,
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::repo_source::is_github_repo_link;

#[derive(Debug, Clone, Parser)]
#[command(name = "brf")]
#[command(about = "Terminal-native change impact simulator")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
    #[arg(value_name = "REPO_PATH_OR_GITHUB_URL")]
    pub repo_path: Option<PathBuf>,
    #[arg(long, default_value_t = 200_000)]
    pub max_file_bytes: u64,
    #[arg(long = "ignore")]
    pub ignore: Vec<String>,
    #[arg(long, help = "Report output directory. Defaults to target repo root")]
    pub output_dir: Option<PathBuf>,
    #[arg(long, default_value = "gpt-5-mini")]
    pub text_model: String,
    #[arg(long, value_enum, default_value_t = ReasoningEffort::Low)]
    pub reasoning_effort: ReasoningEffort,
    #[arg(long, default_value_t = 8_000)]
    pub max_output_tokens: u32,
    #[arg(long, default_value_t = 80)]
    pub max_prompt_files: usize,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CliCommand {
    #[command(about = "Scan a repository and emit a versioned JSON envelope")]
    Scan(ScanCommand),
    #[command(about = "Analyze a change request and emit a versioned JSON envelope")]
    Analyze(AnalyzeCommand),
    #[command(about = "Report the blast radius of the working-tree diff as JSON")]
    Blast(BlastCommand),
    #[command(about = "Watch the working-tree diff and show its blast radius live")]
    Watch(WatchCommand),
}

#[derive(Debug, Clone, Args)]
pub struct BlastCommand {
    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
    #[arg(value_name = "REPO_PATH")]
    pub repo_path: PathBuf,
    #[arg(
        long = "path",
        help = "Treat this file as changed (proposed change); repeatable"
    )]
    pub path: Vec<String>,
    #[arg(long, default_value_t = 200_000)]
    pub max_file_bytes: u64,
    #[arg(long = "ignore")]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct WatchCommand {
    #[arg(value_name = "REPO_PATH")]
    pub repo_path: PathBuf,
    #[arg(long, default_value_t = 1000)]
    pub interval_ms: u64,
    #[arg(long, default_value_t = 200_000)]
    pub max_file_bytes: u64,
    #[arg(long = "ignore")]
    pub ignore: Vec<String>,
    #[arg(long, default_value = "gpt-5-mini")]
    pub text_model: String,
    #[arg(long, value_enum, default_value_t = ReasoningEffort::Low)]
    pub reasoning_effort: ReasoningEffort,
    #[arg(long, default_value_t = 8_000)]
    pub max_output_tokens: u32,
    #[arg(long, default_value_t = 80)]
    pub max_prompt_files: usize,
}

#[derive(Debug, Clone, Args)]
pub struct ScanCommand {
    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
    #[arg(value_name = "REPO_PATH_OR_GITHUB_URL")]
    pub repo_path: PathBuf,
    #[arg(long, default_value_t = 200_000)]
    pub max_file_bytes: u64,
    #[arg(long = "ignore")]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct AnalyzeCommand {
    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
    #[arg(value_name = "REPO_PATH_OR_GITHUB_URL")]
    pub repo_path: PathBuf,
    #[arg(long, help = "Natural-language change request to analyze")]
    pub change: String,
    #[arg(long, default_value_t = 200_000)]
    pub max_file_bytes: u64,
    #[arg(long = "ignore")]
    pub ignore: Vec<String>,
    #[arg(long, default_value = "gpt-5-mini")]
    pub text_model: String,
    #[arg(long, value_enum, default_value_t = ReasoningEffort::Low)]
    pub reasoning_effort: ReasoningEffort,
    #[arg(long, default_value_t = 8_000)]
    pub max_output_tokens: u32,
    #[arg(long, default_value_t = 80)]
    pub max_prompt_files: usize,
}

impl Cli {
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    pub fn parse_from<I, T>(itr: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        <Self as Parser>::parse_from(itr)
    }

    pub fn resolved_model(&self) -> String {
        env::var("BRANCH_TO_THE_FUTURE_MODEL").unwrap_or_else(|_| self.text_model.clone())
    }

    pub fn resolved_output_dir(&self) -> PathBuf {
        self.output_dir
            .clone()
            .or_else(|| self.repo_path.clone())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn resolved_output_dir_for(&self, local_repo_path: &Path) -> PathBuf {
        self.output_dir
            .clone()
            .unwrap_or_else(|| local_repo_path.to_path_buf())
    }

    pub fn tui_repo_path(&self) -> Result<&Path> {
        self.repo_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repo path is required for interactive TUI mode"))
    }
}

impl AnalyzeCommand {
    pub fn resolved_model(&self) -> String {
        env::var("BRANCH_TO_THE_FUTURE_MODEL").unwrap_or_else(|_| self.text_model.clone())
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
    Xhigh,
}

impl ReasoningEffort {
    pub fn as_api_value(self) -> Option<&'static str> {
        match self {
            ReasoningEffort::None => None,
            ReasoningEffort::Low => Some("low"),
            ReasoningEffort::Medium => Some("medium"),
            ReasoningEffort::High => Some("high"),
            ReasoningEffort::Xhigh => Some("xhigh"),
        }
    }
}

pub fn validate_startup(cli: &Cli, api_key: Option<&str>) -> Result<()> {
    validate_repo_input(cli.tui_repo_path()?)?;
    if api_key.map(str::trim).unwrap_or_default().is_empty() {
        bail!("OPENAI_API_KEY is required before analysis");
    }
    Ok(())
}

pub fn validate_repo_input(repo_path: &Path) -> Result<()> {
    if !is_github_repo_link(repo_path) {
        if !repo_path.exists() {
            bail!("repo path does not exist: {}", repo_path.display());
        }
        if !repo_path.is_dir() {
            bail!("repo path is not a directory: {}", repo_path.display());
        }
    }
    Ok(())
}

pub fn load_openai_api_key(cli: &Cli) -> Result<String> {
    let search_dirs = dotenv_search_dirs(cli)?;
    resolve_openai_api_key(env::var("OPENAI_API_KEY").ok().as_deref(), &search_dirs)
}

pub fn dotenv_search_dirs(cli: &Cli) -> Result<Vec<PathBuf>> {
    dotenv_search_dirs_for(cli.tui_repo_path()?)
}

pub fn dotenv_search_dirs_for(repo_path: &Path) -> Result<Vec<PathBuf>> {
    let cwd = env::current_dir()?;
    let mut search_dirs = vec![cwd.clone()];
    if repo_path.exists() && repo_path != cwd {
        search_dirs.push(repo_path.to_path_buf());
    }
    Ok(search_dirs)
}

pub fn resolve_openai_api_key(
    env_api_key: Option<&str>,
    search_dirs: &[PathBuf],
) -> Result<String> {
    if let Some(value) = non_empty(env_api_key) {
        return Ok(value.to_string());
    }

    for dir in search_dirs {
        if let Some(value) = read_dotenv_openai_api_key(&dir.join(".env"))? {
            return Ok(value);
        }
    }

    Ok(String::new())
}

fn read_dotenv_openai_api_key(path: &Path) -> Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    for line in content.lines() {
        let line = line.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "OPENAI_API_KEY" {
            if let Some(value) = non_empty(Some(value)) {
                return Ok(Some(strip_quotes(value).to_string()));
            }
        }
    }

    Ok(None)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn strip_quotes(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}
