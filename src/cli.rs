use std::{
    env,
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(name = "brf")]
#[command(about = "Terminal-native change impact simulator")]
pub struct Cli {
    pub repo_path: PathBuf,
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
    #[arg(long, default_value_t = 2_500)]
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
        env::var("BRANCH_FUTURES_MODEL").unwrap_or_else(|_| self.text_model.clone())
    }

    pub fn resolved_output_dir(&self) -> PathBuf {
        self.output_dir
            .clone()
            .unwrap_or_else(|| self.repo_path.clone())
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
    if !cli.repo_path.exists() {
        bail!("repo path does not exist: {}", cli.repo_path.display());
    }
    if !cli.repo_path.is_dir() {
        bail!("repo path is not a directory: {}", cli.repo_path.display());
    }
    if api_key.map(str::trim).unwrap_or_default().is_empty() {
        bail!("OPENAI_API_KEY is required before analysis");
    }
    Ok(())
}

pub fn load_openai_api_key(cli: &Cli) -> Result<String> {
    let cwd = env::current_dir()?;
    let search_dirs = if cwd == cli.repo_path {
        vec![cwd]
    } else {
        vec![cwd, cli.repo_path.clone()]
    };
    resolve_openai_api_key(env::var("OPENAI_API_KEY").ok().as_deref(), &search_dirs)
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
