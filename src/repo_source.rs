use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
    pub clone_url: String,
    pub label: String,
}

impl GitHubRepo {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        let input = input
            .split(['?', '#'])
            .next()
            .unwrap_or(input)
            .trim_end_matches('/');
        let Some(path) = input
            .strip_prefix("https://github.com/")
            .or_else(|| input.strip_prefix("http://github.com/"))
        else {
            bail!("not a GitHub repo link");
        };
        let mut parts = path.split('/').filter(|part| !part.is_empty());
        let Some(owner) = parts.next() else {
            bail!("GitHub repo link is missing owner");
        };
        let Some(name) = parts.next() else {
            bail!("GitHub repo link is missing repo name");
        };
        let name = name.strip_suffix(".git").unwrap_or(name);
        if owner.is_empty() || name.is_empty() {
            bail!("GitHub repo link is missing owner or repo name");
        }
        let clone_url = format!("https://github.com/{owner}/{name}.git");
        let label = format!("github.com/{owner}/{name}");
        Ok(Self {
            owner: owner.to_string(),
            name: name.to_string(),
            clone_url,
            label,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRepo {
    pub local_path: PathBuf,
    pub source_label: String,
    pub temporary_clone: bool,
}

impl PreparedRepo {
    pub fn local(path: PathBuf) -> Self {
        Self {
            source_label: path.display().to_string(),
            local_path: path,
            temporary_clone: false,
        }
    }
}

pub fn is_github_repo_link(input: &Path) -> bool {
    GitHubRepo::parse(&input.to_string_lossy()).is_ok()
}

pub fn prepare_repo_source(input: &Path, search_dirs: &[PathBuf]) -> Result<PreparedRepo> {
    let token = resolve_github_token(
        env::var("GITHUB_TOKEN").ok().as_deref(),
        env::var("GH_TOKEN").ok().as_deref(),
        search_dirs,
    )?;
    prepare_repo_source_with_cloner(input, &env::temp_dir(), token.as_deref(), clone_github_repo)
}

#[doc(hidden)]
pub fn prepare_repo_source_with_cloner<F>(
    input: &Path,
    clone_root: &Path,
    token: Option<&str>,
    mut clone: F,
) -> Result<PreparedRepo>
where
    F: FnMut(&GitHubRepo, &Path, Option<&str>) -> Result<()>,
{
    let input_text = input.to_string_lossy();
    let Ok(repo) = GitHubRepo::parse(&input_text) else {
        return Ok(PreparedRepo::local(input.to_path_buf()));
    };
    let destination = temp_clone_path(clone_root, &repo);
    clone(&repo, &destination, token)?;
    Ok(PreparedRepo {
        local_path: destination,
        source_label: repo.label,
        temporary_clone: true,
    })
}

pub fn resolve_github_token(
    env_github_token: Option<&str>,
    env_gh_token: Option<&str>,
    search_dirs: &[PathBuf],
) -> Result<Option<String>> {
    if let Some(value) = non_empty(env_github_token).or_else(|| non_empty(env_gh_token)) {
        return Ok(Some(value.to_string()));
    }

    for dir in search_dirs {
        let path = dir.join(".env");
        for key in [
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GITHUB_PAT",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
        ] {
            if let Some(value) = read_dotenv_value(&path, key)? {
                return Ok(Some(value));
            }
        }
    }

    Ok(None)
}

fn clone_github_repo(repo: &GitHubRepo, destination: &Path, token: Option<&str>) -> Result<()> {
    let parent = destination
        .parent()
        .context("temp clone destination has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let mut command = Command::new("git");
    command
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&repo.clone_url)
        .arg(destination)
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(token) = token {
        command
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader")
            .env("GIT_CONFIG_VALUE_0", github_authorization_header(token));
    }

    let output = command
        .output()
        .with_context(|| format!("failed to run git clone for {}", repo.label))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{}",
            github_clone_error_message(&repo.label, stderr.trim(), token.is_some())
        );
    }
    Ok(())
}

pub fn github_clone_error_message(label: &str, stderr: &str, token_loaded: bool) -> String {
    let mut message = format!("git clone failed for {label}: {stderr}");
    if !token_loaded && stderr.contains("could not read Username") {
        message.push_str(
            "\nNo GitHub token was loaded. Add GITHUB_TOKEN, GH_TOKEN, or GITHUB_PAT to .env in the directory where you run brf, or export it in your shell.",
        );
    }
    message
}

pub fn github_authorization_header(token: &str) -> String {
    format!(
        "AUTHORIZATION: Basic {}",
        base64_encode(format!("x-access-token:{token}").as_bytes())
    )
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        let triple = ((first as u32) << 16) | ((second as u32) << 8) | third as u32;

        output.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

/// Process-unique hex id built from wall-clock nanos, the process id and an
/// atomic counter. The counter keeps ids distinct for concurrent callers in
/// one process; the pid and nanos separate processes.
pub fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{:x}-{count:x}", std::process::id())
}

fn temp_clone_path(root: &Path, repo: &GitHubRepo) -> PathBuf {
    root.join("branch-to-the-future-clones").join(format!(
        "{}-{}-{}",
        safe_segment(&repo.owner),
        safe_segment(&repo.name),
        unique_id()
    ))
}

fn safe_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn read_dotenv_value(path: &Path, key: &str) -> Result<Option<String>> {
    let content = match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    for line in content.lines() {
        let line = line.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((candidate, value)) = line.split_once('=') else {
            continue;
        };
        if candidate.trim() == key {
            let value = parse_dotenv_value(value);
            if !value.is_empty() {
                return Ok(Some(value));
            }
        }
    }

    Ok(None)
}

/// Parses the right-hand side of a `KEY=value` line: quoted values are
/// unquoted (a trailing comment after the closing quote is ignored); unquoted
/// values end at the first ` #` or tab-`#`.
fn parse_dotenv_value(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(quote) = raw.chars().next().filter(|c| *c == '"' || *c == '\'') {
        if let Some(end) = raw[1..].find(quote) {
            return raw[1..1 + end].to_string();
        }
    }
    let mut end = raw.len();
    for (index, ch) in raw.char_indices() {
        if ch == '#' && (index == 0 || raw[..index].ends_with([' ', '\t'])) {
            end = index;
            break;
        }
    }
    raw[..end].trim().to_string()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod dotenv_tests {
    use super::*;

    fn read(content: &[u8]) -> Option<String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, content).unwrap();
        read_dotenv_value(&path, "KEY").unwrap()
    }

    #[test]
    fn strips_inline_comments_and_quotes() {
        assert_eq!(read(b"KEY=abc # comment\n").as_deref(), Some("abc"));
        assert_eq!(read(b"KEY=\"abc\" # c\n").as_deref(), Some("abc"));
        assert_eq!(read(b"KEY='a b' # c\n").as_deref(), Some("a b"));
        assert_eq!(
            read(b"export KEY=abc#notacomment\n").as_deref(),
            Some("abc#notacomment")
        );
        assert_eq!(read(b"KEY=\"a # b\"\n").as_deref(), Some("a # b"));
        assert_eq!(read(b"KEY= # only comment\n"), None);
    }

    #[test]
    fn non_utf8_env_file_does_not_abort() {
        assert_eq!(read(b"BAD=\xff\xfe\nKEY=abc\n").as_deref(), Some("abc"));
    }
}

#[cfg(test)]
mod unique_id_tests {
    use super::unique_id;

    #[test]
    fn unique_ids_are_distinct_across_threads() {
        let handles: Vec<_> = (0..8)
            .map(|_| std::thread::spawn(|| (0..500).map(|_| unique_id()).collect::<Vec<_>>()))
            .collect();
        let mut all = std::collections::HashSet::new();
        for handle in handles {
            for id in handle.join().unwrap() {
                assert!(all.insert(id));
            }
        }
        assert_eq!(all.len(), 4000);
    }
}
