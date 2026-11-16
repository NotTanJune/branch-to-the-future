use std::{
    env, fmt,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    str::FromStr,
};

use regex::Regex;
use serde_json::{json, Value};
use thiserror::Error;

const SETUP_HELP: &str =
    "Set `NOTION_TOKEN` or `NOTION_API_KEY` with insert_content and update_content capabilities, or install Notion CLI with `curl -fsSL https://ntn.dev | bash` and run `ntn login`.";
const NOTION_API_VERSION: &str = "2026-03-11";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotionParentRef {
    Page(String),
    Database(String),
    DataSource(String),
}

impl NotionParentRef {
    fn parent_json(&self) -> Value {
        match self {
            Self::Page(id) => json!({
                "type": "page_id",
                "page_id": id,
            }),
            Self::Database(id) => json!({
                "type": "database_id",
                "database_id": id,
            }),
            Self::DataSource(id) => json!({
                "type": "data_source_id",
                "data_source_id": id,
            }),
        }
    }
}

impl FromStr for NotionParentRef {
    type Err = NotionExportError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(NotionExportError::InvalidParentRef(
                "Notion parent ref is empty".to_string(),
            ));
        }

        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("page:") {
            return Ok(Self::Page(canonical_notion_id(rest)?));
        }
        if let Some(rest) = lower.strip_prefix("database:") {
            return Ok(Self::Database(canonical_notion_id(rest)?));
        }
        if let Some(rest) = lower
            .strip_prefix("data-source:")
            .or_else(|| lower.strip_prefix("data_source:"))
        {
            return Ok(Self::DataSource(canonical_notion_id(rest)?));
        }

        Ok(Self::Page(canonical_notion_id(trimmed)?))
    }
}

impl fmt::Display for NotionParentRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Page(id) => write!(f, "page:{id}"),
            Self::Database(id) => write!(f, "database:{id}"),
            Self::DataSource(id) => write!(f, "data-source:{id}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NotionExportRequest {
    pub parent: Option<NotionParentRef>,
    pub page_id: Option<String>,
    pub page_url: Option<String>,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotionExportResult {
    pub page_id: String,
    pub page_url: String,
    pub action: NotionExportAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotionExportAction {
    Created,
    Updated,
}

#[derive(Debug, Error)]
pub enum NotionExportError {
    #[error("Notion CLI `ntn` was not found. {SETUP_HELP}")]
    MissingCli,
    #[error("Could not start Notion CLI: {0}. {SETUP_HELP}")]
    CommandLaunch(String),
    #[error("Notion CLI failed: {stderr}. {SETUP_HELP}")]
    CommandFailed { stderr: String },
    #[error("Invalid Notion parent ref: {0}")]
    InvalidParentRef(String),
    #[error("Invalid Notion export request: {0}")]
    InvalidRequest(String),
    #[error("Invalid Notion CLI response: {0}")]
    InvalidResponse(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn export_to_notion(
    request: NotionExportRequest,
) -> Result<NotionExportResult, NotionExportError> {
    let token = notion_api_token_from_env();
    export_to_notion_with_auth(Path::new("ntn"), token.as_deref(), request)
}

pub fn notion_api_token_from_env() -> Option<String> {
    env::var("NOTION_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("NOTION_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

pub(crate) fn export_to_notion_with_auth(
    ntn_bin: &Path,
    api_token: Option<&str>,
    request: NotionExportRequest,
) -> Result<NotionExportResult, NotionExportError> {
    match (&request.parent, &request.page_id) {
        (Some(_), None) => create_page(ntn_bin, api_token, request),
        (None, Some(_)) => update_page(ntn_bin, api_token, request),
        (Some(_), Some(_)) => Err(NotionExportError::InvalidRequest(
            "provide parent for create or page_id for update, not both".to_string(),
        )),
        (None, None) => Err(NotionExportError::InvalidRequest(
            "provide parent for create or page_id for update".to_string(),
        )),
    }
}

fn create_page(
    ntn_bin: &Path,
    api_token: Option<&str>,
    request: NotionExportRequest,
) -> Result<NotionExportResult, NotionExportError> {
    let parent = request
        .parent
        .ok_or_else(|| NotionExportError::InvalidRequest("create requires parent".to_string()))?;
    let body = json!({
        "parent": parent.parent_json(),
        "markdown": request.markdown,
    });
    let response = run_ntn_api(ntn_bin, &["api", "v1/pages"], &body, api_token)?;
    let page_id = response
        .get("id")
        .and_then(Value::as_str)
        .map(canonical_notion_id)
        .transpose()?
        .ok_or_else(|| NotionExportError::InvalidResponse("missing page id".to_string()))?;
    let page_url = response
        .get("url")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| notion_page_url(&page_id));

    Ok(NotionExportResult {
        page_id,
        page_url,
        action: NotionExportAction::Created,
    })
}

fn update_page(
    ntn_bin: &Path,
    api_token: Option<&str>,
    request: NotionExportRequest,
) -> Result<NotionExportResult, NotionExportError> {
    let page_id = request
        .page_id
        .as_deref()
        .map(canonical_notion_id)
        .transpose()?
        .ok_or_else(|| NotionExportError::InvalidRequest("update requires page_id".to_string()))?;
    let body = json!({
        "type": "replace_content",
        "replace_content": {
            "new_str": request.markdown,
        },
    });
    let path = format!("v1/pages/{page_id}/markdown");
    let response = run_ntn_api(ntn_bin, &["api", &path, "-X", "PATCH"], &body, api_token)?;
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .map(canonical_notion_id)
        .transpose()?
        .unwrap_or_else(|| page_id.clone());
    let page_url = request
        .page_url
        .unwrap_or_else(|| notion_page_url(&response_id));

    Ok(NotionExportResult {
        page_id: response_id,
        page_url,
        action: NotionExportAction::Updated,
    })
}

fn run_ntn_api(
    ntn_bin: &Path,
    args: &[&str],
    body: &Value,
    api_token: Option<&str>,
) -> Result<Value, NotionExportError> {
    let mut command = Command::new(ntn_bin);
    command.args(args);
    if env::var_os("NOTION_API_VERSION").is_none() {
        command.env("NOTION_API_VERSION", NOTION_API_VERSION);
    }
    if let Some(token) = api_token.filter(|token| !token.trim().is_empty()) {
        command.env("NOTION_API_TOKEN", token);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                NotionExportError::MissingCli
            } else {
                NotionExportError::CommandLaunch(error.to_string())
            }
        })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| NotionExportError::CommandLaunch("failed to open stdin".to_string()))?;
    let payload = serde_json::to_vec(body)?;
    // Write on a separate thread so a child that exits early (or fills its
    // stdout pipe) cannot deadlock or turn its real error into a broken pipe.
    // A write error is ignored: the exit status and stderr carry the cause.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&payload);
    });

    let output = child
        .wait_with_output()
        .map_err(|error| NotionExportError::CommandLaunch(error.to_string()));
    let _ = writer.join();
    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(NotionExportError::CommandFailed {
            stderr: if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            },
        });
    }

    serde_json::from_slice(&output.stdout).map_err(NotionExportError::from)
}

fn canonical_notion_id(input: &str) -> Result<String, NotionExportError> {
    let source = input.split('?').next().unwrap_or(input);
    let re = Regex::new(
        r"(?i)([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[0-9a-f]{32})",
    )
    .expect("valid Notion ID regex");
    let Some(id) = re.find_iter(source).last().map(|match_| match_.as_str()) else {
        return Err(NotionExportError::InvalidParentRef(input.to_string()));
    };
    let compact = id.replace('-', "").to_ascii_lowercase();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &compact[0..8],
        &compact[8..12],
        &compact[12..16],
        &compact[16..20],
        &compact[20..32]
    ))
}

fn notion_page_url(page_id: &str) -> String {
    format!("https://www.notion.so/{}", page_id.replace('-', ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    #[test]
    fn token_auth_still_uses_ntn_api_for_create() {
        let dir = tempfile::tempdir().unwrap();
        let result = export_to_notion_with_auth(
            &mock_ntn(dir.path()),
            Some("secret-token"),
            NotionExportRequest {
                parent: Some(NotionParentRef::Page(
                    "12345678-90ab-cdef-1234-567890abcdef".to_string(),
                )),
                page_id: None,
                page_url: None,
                markdown: "# Report\n\nBody".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("args")).unwrap(),
            "api\nv1/pages\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("token")).unwrap(),
            "secret-token\n"
        );
        let body = fs::read_to_string(dir.path().join("stdin.json")).unwrap();
        assert!(body.contains(r#""page_id":"12345678-90ab-cdef-1234-567890abcdef""#));
        assert!(body.contains(r##""markdown":"# Report\n\nBody""##));
        assert_eq!(result.action, NotionExportAction::Created);
        assert_eq!(result.page_id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    }

    #[test]
    fn token_auth_still_uses_ntn_api_for_update_without_deleting_children() {
        let dir = tempfile::tempdir().unwrap();
        let result = export_to_notion_with_auth(
            &mock_ntn(dir.path()),
            Some("secret-token"),
            NotionExportRequest {
                parent: None,
                page_id: Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string()),
                page_url: Some("https://www.notion.so/page".to_string()),
                markdown: "# Updated".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("args")).unwrap(),
            "api\nv1/pages/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/markdown\n-X\nPATCH\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("token")).unwrap(),
            "secret-token\n"
        );
        let body = fs::read_to_string(dir.path().join("stdin.json")).unwrap();
        assert!(body.contains(r#""type":"replace_content""#));
        assert!(body.contains(r##""new_str":"# Updated""##));
        assert!(!body.contains("allow_deleting_content"));
        assert_eq!(result.action, NotionExportAction::Updated);
        assert_eq!(result.page_url, "https://www.notion.so/page");
    }

    #[test]
    fn early_exiting_ntn_reports_its_stderr_not_broken_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("ntn");
        fs::write(
            &script,
            "#!/bin/sh\necho 'auth failed: bad token' >&2\nexit 3\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let error = export_to_notion_with_auth(
            &script,
            None,
            NotionExportRequest {
                parent: Some(NotionParentRef::Page(
                    "12345678-90ab-cdef-1234-567890abcdef".to_string(),
                )),
                page_id: None,
                page_url: None,
                markdown: "x".repeat(8 * 1024 * 1024),
            },
        )
        .unwrap_err();

        match error {
            NotionExportError::CommandFailed { stderr } => {
                assert!(stderr.contains("auth failed: bad token"), "{stderr}")
            }
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    fn mock_ntn(dir: &Path) -> PathBuf {
        let script = dir.join("ntn");
        let script_body = format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > '{args}'
printf '%s\n' "$NOTION_API_TOKEN" > '{token}'
cat > '{stdin}'
case "$*" in
  *"/markdown"*) printf '%s\n' '{{"object":"page_markdown","id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","markdown":"ok","truncated":false,"unknown_block_ids":[]}}' ;;
  *) printf '%s\n' '{{"object":"page","id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","url":"https://www.notion.so/Branch-aaaaaaaaaaaaaaaabbbbccccddddeeee"}}' ;;
esac
"#,
            args = dir.join("args").display(),
            token = dir.join("token").display(),
            stdin = dir.join("stdin.json").display()
        );
        fs::write(&script, script_body).unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        script
    }
}
