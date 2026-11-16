use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH,
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    cli::BlastCommand,
    domain::{RepoModel, RiskSignal, RouteInfo},
    machine::created_at_rfc3339,
    repo::scan_repo,
    repo_source::is_github_repo_link,
};

const SCHEMA_VERSION: u8 = 1;
const TOOL_NAME: &str = "branch-to-the-future";
const JS_EXTS: [&str; 6] = ["ts", "tsx", "js", "jsx", "mjs", "cjs"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlastImpact {
    pub path: String,
    pub depth: u8,
    pub via: Vec<String>,
    pub is_test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlastReport {
    pub schema_version: u8,
    pub tool: String,
    pub kind: String,
    pub created_at: String,
    pub repo_root: String,
    pub changed: Vec<ChangedFile>,
    pub impacted: Vec<BlastImpact>,
    pub tests: Vec<String>,
    pub untested_changes: Vec<String>,
    pub routes: Vec<RouteInfo>,
    pub risk_signals: Vec<RiskSignal>,
    pub sensitive: Vec<String>,
    pub score: u8,
    pub level: String,
    pub reasons: Vec<String>,
}

// ---------------------------------------------------------------- git

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<String> {
    let output = git(root, args)?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn has_head(root: &Path) -> bool {
    git(root, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Fails with a short message when `root` is not inside a git work tree.
pub fn ensure_git_repo(root: &Path) -> Result<()> {
    let inside = git(root, &["rev-parse", "--is-inside-work-tree"])
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true"
        })
        .unwrap_or(false);
    if !inside {
        bail!("not a git repository: {}", root.display());
    }
    Ok(())
}

/// Parses `git diff --name-status -z` output (NUL separated, no path quoting).
/// A rename is reported as the old path deleted plus the new path renamed so
/// importers of the old path are still impacted.
fn parse_name_status(text: &str, out: &mut Vec<ChangedFile>, seen: &mut HashSet<String>) {
    let mut push = |path: &str, status: &str| {
        if !path.is_empty() && seen.insert(path.to_string()) {
            out.push(ChangedFile {
                path: path.to_string(),
                status: status.to_string(),
            });
        }
    };
    let mut tokens = text.split('\0');
    while let Some(code) = tokens.next() {
        let code = code.trim_start_matches('\n');
        let Some(kind) = code.chars().next() else {
            continue;
        };
        match kind {
            'R' | 'C' => {
                let (Some(old), Some(new)) = (tokens.next(), tokens.next()) else {
                    break;
                };
                if kind == 'R' {
                    push(old, "deleted");
                    push(new, "renamed");
                } else {
                    push(new, "added");
                }
            }
            'M' | 'T' | 'A' | 'D' => {
                let Some(path) = tokens.next() else { break };
                let status = match kind {
                    'M' | 'T' => "modified",
                    'A' => "added",
                    _ => "deleted",
                };
                push(path, status);
            }
            _ => {
                // Unknown record: skip its path token.
                tokens.next();
            }
        }
    }
}

/// Changed files relative to `root`, even when `root` is a subdirectory of the
/// git work tree (`--relative` restricts to the subtree and strips the prefix).
pub fn git_changed_files(root: &Path) -> Result<Vec<ChangedFile>> {
    ensure_git_repo(root)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if has_head(root) {
        let text = git_stdout(root, &["diff", "--relative", "--name-status", "-z", "HEAD"])?;
        parse_name_status(&text, &mut out, &mut seen);
    } else {
        let cached = git_stdout(
            root,
            &["diff", "--relative", "--name-status", "-z", "--cached"],
        )?;
        parse_name_status(&cached, &mut out, &mut seen);
        let worktree = git_stdout(root, &["diff", "--relative", "--name-status", "-z"])?;
        parse_name_status(&worktree, &mut out, &mut seen);
    }
    for path in untracked_files(root)? {
        if seen.insert(path.clone()) {
            out.push(ChangedFile {
                path,
                status: "untracked".to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn untracked_files(root: &Path) -> Result<Vec<String>> {
    let text = git_stdout(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    Ok(text
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect())
}

/// Text of the working-tree diff against HEAD (or index/worktree when no HEAD).
pub fn git_diff_text(root: &Path) -> Result<String> {
    if has_head(root) {
        git_stdout(root, &["diff", "--relative", "HEAD"])
    } else {
        let mut text = git_stdout(root, &["diff", "--relative", "--cached"])?;
        text.push_str(&git_stdout(root, &["diff", "--relative"])?);
        Ok(text)
    }
}

pub fn untracked_list(root: &Path) -> Vec<String> {
    untracked_files(root).unwrap_or_default()
}

pub fn diff_fingerprint(root: &Path) -> Result<u64> {
    let mut hasher = DefaultHasher::new();
    git_diff_text(root)?.hash(&mut hasher);
    for path in untracked_files(root)? {
        let (len, mtime) = fs::metadata(root.join(&path))
            .map(|meta| {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|dur| dur.as_nanos())
                    .unwrap_or(0);
                (meta.len(), mtime)
            })
            .unwrap_or((0, 0));
        path.hash(&mut hasher);
        len.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    Ok(hasher.finish())
}

// ---------------------------------------------------------------- import resolution

/// Resolves raw import strings to repo file paths.
pub struct ImportResolver {
    paths: BTreeSet<String>,
    names: HashSet<String>,
}

impl ImportResolver {
    pub fn new<I: IntoIterator<Item = String>>(paths: I) -> Self {
        let paths: BTreeSet<String> = paths.into_iter().collect();
        let names = paths
            .iter()
            .filter_map(|path| path.rsplit('/').next().map(str::to_string))
            .collect();
        Self { paths, names }
    }

    pub fn targets(&self, importer: &str, import: &str) -> Vec<String> {
        let ext = file_ext(importer);
        let mut found = match ext.as_str() {
            "py" => self.python(importer, import),
            "rs" => self.rust(importer, import),
            e if JS_EXTS.contains(&e) => self.js(importer, import),
            _ => Vec::new(),
        };
        found.retain(|target| target != importer);
        found.dedup();
        found
    }

    fn has(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    fn js(&self, importer: &str, spec: &str) -> Vec<String> {
        if spec.starts_with("./") || spec.starts_with("../") || spec == "." || spec == ".." {
            let Some(base) = normalize(&join(dir_of(importer), spec)) else {
                return Vec::new();
            };
            return self.js_candidates(&base).into_iter().take(1).collect();
        }
        for prefix in ["@/", "~/"] {
            if let Some(rest) = spec.strip_prefix(prefix) {
                let rest = rest.trim_end_matches('/');
                let mut hits = Vec::new();
                for path in &self.paths {
                    if !JS_EXTS.contains(&file_ext(path).as_str()) {
                        continue;
                    }
                    let stem = strip_ext(path);
                    let stem_index = stem.strip_suffix("/index").unwrap_or(stem);
                    if [stem, stem_index]
                        .iter()
                        .any(|s| *s == rest || s.ends_with(&format!("/{rest}")))
                    {
                        hits.push(path.clone());
                    }
                }
                return hits;
            }
        }
        Vec::new()
    }

    fn js_candidates(&self, base: &str) -> Vec<String> {
        let mut bases = vec![base.to_string()];
        if JS_EXTS.contains(&file_ext(base).as_str()) {
            bases.push(strip_ext(base).to_string());
        }
        let mut tries = Vec::new();
        if JS_EXTS.contains(&file_ext(base).as_str()) {
            tries.push(base.to_string());
        }
        for b in &bases {
            for ext in JS_EXTS {
                tries.push(format!("{b}.{ext}"));
            }
            for ext in JS_EXTS {
                tries.push(format!("{b}/index.{ext}"));
            }
        }
        tries.into_iter().filter(|c| self.has(c)).collect()
    }

    fn python(&self, importer: &str, import: &str) -> Vec<String> {
        let dots = import.chars().take_while(|c| *c == '.').count();
        let module = &import[dots..];
        let rel = module.replace('.', "/");
        let mut candidates = Vec::new();
        if dots > 0 {
            let mut base = dir_of(importer).to_string();
            for _ in 1..dots {
                base = dir_of(&base).to_string();
            }
            let joined = join(&base, &rel);
            if rel.is_empty() {
                candidates.push(join(&base, "__init__.py"));
            } else {
                candidates.push(format!("{joined}.py"));
                candidates.push(format!("{joined}/__init__.py"));
            }
        } else if !rel.is_empty() {
            for prefix in ["", "src/"] {
                candidates.push(format!("{prefix}{rel}.py"));
                candidates.push(format!("{prefix}{rel}/__init__.py"));
            }
        }
        let exact: Vec<String> = candidates.iter().filter(|c| self.has(c)).cloned().collect();
        if !exact.is_empty() || dots > 0 || rel.is_empty() {
            return exact.into_iter().take(1).collect();
        }
        // Suffix fallback for nested package roots (python/pkg/mod.py).
        let wants = [format!("{rel}.py"), format!("{rel}/__init__.py")];
        let base_names: Vec<&str> = wants.iter().filter_map(|w| w.rsplit('/').next()).collect();
        if !base_names.iter().any(|n| self.names.contains(*n)) {
            return Vec::new();
        }
        for want in &wants {
            let suffix = format!("/{want}");
            if let Some(hit) = self.paths.iter().find(|p| p.ends_with(&suffix)) {
                return vec![hit.clone()];
            }
        }
        Vec::new()
    }

    fn rust(&self, importer: &str, import: &str) -> Vec<String> {
        let segs: Vec<&str> = import.split("::").filter(|s| !s.is_empty()).collect();
        let Some(first) = segs.first().copied() else {
            return Vec::new();
        };
        if matches!(first, "std" | "core" | "alloc") {
            return Vec::new();
        }
        let root = rust_src_root(importer);
        match first {
            "crate" => self.rust_module(&root, &segs[1..]),
            "self" => self.rust_module(&rust_module_dir(importer), &segs[1..]),
            "super" => {
                let mut dir = rust_module_dir(importer);
                let mut idx = 0;
                while idx < segs.len() && segs[idx] == "super" {
                    dir = dir_of(&dir).to_string();
                    idx += 1;
                }
                self.rust_module(&dir, &segs[idx..])
            }
            _ => {
                let hit = self.rust_module(&root, &segs[1..]);
                if hit.is_empty() {
                    self.rust_module(&root, &segs)
                } else {
                    hit
                }
            }
        }
    }

    fn rust_module(&self, dir: &str, segs: &[&str]) -> Vec<String> {
        let dir = dir.trim_end_matches('/');
        for k in (1..=segs.len()).rev() {
            let rel = segs[..k].join("/");
            let base = if dir.is_empty() {
                rel
            } else {
                format!("{dir}/{rel}")
            };
            for cand in [format!("{base}.rs"), format!("{base}/mod.rs")] {
                if self.has(&cand) {
                    return vec![cand];
                }
            }
        }
        Vec::new()
    }
}

/// Convenience wrapper over `ImportResolver` for one-off lookups.
pub fn import_targets(importer: &str, import: &str, paths: &[String]) -> Vec<String> {
    ImportResolver::new(paths.iter().cloned()).targets(importer, import)
}

fn rust_src_root(importer: &str) -> String {
    if let Some(rest) = importer.strip_prefix("src/") {
        let _ = rest;
        return "src".to_string();
    }
    if let Some(idx) = importer.find("/src/") {
        return importer[..idx + 4].to_string();
    }
    for marker in ["tests/", "examples/", "benches/"] {
        if let Some(rest) = importer.strip_prefix(marker) {
            let _ = rest;
            return "src".to_string();
        }
        if let Some(idx) = importer.find(&format!("/{marker}")) {
            return format!("{}/src", &importer[..idx]);
        }
    }
    "src".to_string()
}

fn rust_module_dir(importer: &str) -> String {
    let parent = dir_of(importer);
    let name = importer.rsplit('/').next().unwrap_or(importer);
    if matches!(name, "mod.rs" | "lib.rs" | "main.rs") {
        parent.to_string()
    } else {
        join(parent, strip_ext(name))
    }
}

fn file_ext(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

fn strip_ext(path: &str) -> &str {
    let name_start = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    match path[name_start..].rfind('.') {
        Some(dot) if dot > 0 => &path[..name_start + dot],
        _ => path,
    }
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

// ---------------------------------------------------------------- scoring

const SENSITIVE_WORDS: [&str; 15] = [
    "auth",
    "authentication",
    "authorization",
    "authorize",
    "login",
    "session",
    "token",
    "secret",
    "password",
    "payment",
    "billing",
    "migration",
    "schema",
    "security",
    "permission",
];
const SENSITIVE_FILES: [&str; 7] = [
    "cargo.toml",
    "package.json",
    "cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "uv.lock",
];

/// Splits a path into lowercase word tokens on `/ _ - .` and camelCase boundaries.
fn path_tokens(path: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in path.chars() {
        if ch == '/' || ch == '_' || ch == '-' || ch == '.' || ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        current.extend(ch.to_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_sensitive(path: &str, config_files: &HashSet<&str>) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    path_tokens(path).iter().any(|token| {
        SENSITIVE_WORDS.iter().any(|word| {
            token == word
                || token
                    .strip_suffix('s')
                    .is_some_and(|plural| plural == *word)
        })
    }) || name == ".env"
        || name.starts_with(".env.")
        || SENSITIVE_FILES.contains(&name)
        || lower.starts_with(".github/workflows/")
        || lower.contains("/.github/workflows/")
        || config_files.contains(path)
}

fn test_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = strip_ext(name);
    let stem = stem
        .strip_suffix(".test")
        .or_else(|| stem.strip_suffix(".spec"))
        .or_else(|| stem.strip_suffix("_test"))
        .or_else(|| stem.strip_suffix("_tests"))
        .unwrap_or(stem);
    stem.strip_prefix("test_").unwrap_or(stem).to_string()
}

fn source_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    strip_ext(name).to_string()
}

pub fn compute_blast(
    model: &RepoModel,
    changed: &[ChangedFile],
    created_at: String,
) -> BlastReport {
    let changed_set: HashSet<&str> = changed.iter().map(|c| c.path.as_str()).collect();
    let test_set: HashSet<&str> = model.tests.iter().map(String::as_str).collect();
    let config_set: HashSet<&str> = model.config_files.iter().map(String::as_str).collect();

    let resolver = ImportResolver::new(
        model
            .files
            .iter()
            .map(|f| f.path.clone())
            .chain(changed.iter().map(|c| c.path.clone())),
    );
    let mut reverse: HashMap<String, BTreeSet<String>> = HashMap::new();
    for file in &model.files {
        for import in &file.imports {
            for target in resolver.targets(&file.path, import) {
                reverse.entry(target).or_default().insert(file.path.clone());
            }
        }
    }

    let mut depth1: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for change in changed {
        if let Some(importers) = reverse.get(&change.path) {
            for importer in importers {
                if !changed_set.contains(importer.as_str()) {
                    depth1
                        .entry(importer.clone())
                        .or_default()
                        .insert(change.path.clone());
                }
            }
        }
    }
    let mut depth2: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for via in depth1.keys() {
        if let Some(importers) = reverse.get(via) {
            for importer in importers {
                if !changed_set.contains(importer.as_str()) && !depth1.contains_key(importer) {
                    depth2
                        .entry(importer.clone())
                        .or_default()
                        .insert(via.clone());
                }
            }
        }
    }

    let mut impacted = Vec::new();
    for (depth, map) in [(1u8, &depth1), (2u8, &depth2)] {
        for (path, via) in map {
            impacted.push(BlastImpact {
                path: path.clone(),
                depth,
                via: via.iter().cloned().collect(),
                is_test: test_set.contains(path.as_str()),
            });
        }
    }
    impacted.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.path.cmp(&b.path)));

    let impacted_set: HashSet<&str> = impacted.iter().map(|i| i.path.as_str()).collect();
    let touched = |path: &str| changed_set.contains(path) || impacted_set.contains(path);

    // Tests: impacted tests, changed tests, or tests whose stem matches a changed source stem.
    let changed_stems: HashMap<String, Vec<&str>> =
        changed.iter().fold(HashMap::new(), |mut acc, c| {
            if !test_set.contains(c.path.as_str()) {
                acc.entry(source_stem(&c.path)).or_default().push(&c.path);
            }
            acc
        });
    let mut tests: BTreeSet<String> = BTreeSet::new();
    let mut tested_stems: HashSet<String> = HashSet::new();
    for test in &model.tests {
        let stem = test_stem(test);
        if changed_stems.contains_key(&stem) {
            tests.insert(test.clone());
            tested_stems.insert(stem);
        }
        if touched(test) {
            tests.insert(test.clone());
        }
    }
    let impacted_by_source: HashSet<&str> = impacted
        .iter()
        .filter(|i| i.is_test)
        .flat_map(|i| i.via.iter().map(String::as_str))
        .collect();
    let untested_changes: Vec<String> = changed
        .iter()
        .filter(|c| c.status != "deleted")
        .filter(|c| !test_set.contains(c.path.as_str()))
        .filter(|c| is_source_path(&c.path))
        .filter(|c| {
            !tested_stems.contains(&source_stem(&c.path))
                && !impacted_by_source.contains(c.path.as_str())
        })
        .map(|c| c.path.clone())
        .collect();

    let routes: Vec<RouteInfo> = model
        .routes
        .iter()
        .filter(|r| touched(&r.path))
        .cloned()
        .collect();
    let risk_signals: Vec<RiskSignal> = model
        .risk_signals
        .iter()
        .filter(|r| touched(&r.path))
        .cloned()
        .collect();
    let sensitive: Vec<String> = changed
        .iter()
        .filter(|c| is_sensitive(&c.path, &config_set))
        .map(|c| c.path.clone())
        .collect();

    let direct = depth1.len();
    let second = depth2.len();
    let mut reasons = Vec::new();
    let raw = if changed.is_empty() {
        reasons.push("no working-tree changes".to_string());
        0.0
    } else {
        let mut by_target: BTreeMap<&str, usize> = BTreeMap::new();
        for via in depth1.values() {
            for v in via {
                *by_target.entry(v.as_str()).or_default() += 1;
            }
        }
        for (target, count) in by_target.iter().take(3) {
            let noun = if *count == 1 {
                "file imports"
            } else {
                "files import"
            };
            reasons.push(format!("{count} {noun} {target}"));
        }
        if second > 0 {
            reasons.push(format!("{second} more files depend on those transitively"));
        }
        if !routes.is_empty() {
            reasons.push(format!("{} routes affected", routes.len()));
        }
        for path in sensitive.iter().take(3) {
            reasons.push(format!("touches sensitive path {path}"));
        }
        if !risk_signals.is_empty() {
            reasons.push(format!(
                "{} risk signals on affected files",
                risk_signals.len()
            ));
        }
        for path in untested_changes.iter().take(3) {
            reasons.push(format!("{path} has no related test"));
        }
        if changed.len() > 5 {
            reasons.push(format!("{} files changed", changed.len()));
        }
        direct as f64
            + second as f64 * 0.5
            + routes.len() as f64
            + sensitive.len() as f64 * 2.0
            + risk_signals.len() as f64
            + untested_changes.len() as f64 * 0.5
            + if changed.len() > 5 { 1.0 } else { 0.0 }
    };
    let score = raw.round().clamp(0.0, 10.0) as u8;
    let level = if score >= 7 {
        "high"
    } else if score >= 4 {
        "medium"
    } else {
        "low"
    };

    BlastReport {
        schema_version: SCHEMA_VERSION,
        tool: TOOL_NAME.to_string(),
        kind: "blast".to_string(),
        created_at,
        repo_root: model.root_path.clone(),
        changed: changed.to_vec(),
        impacted,
        tests: tests.into_iter().collect(),
        untested_changes,
        routes,
        risk_signals,
        sensitive,
        score,
        level: level.to_string(),
        reasons,
    }
}

fn is_source_path(path: &str) -> bool {
    matches!(
        file_ext(path).as_str(),
        "ts" | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "rs"
            | "go"
            | "java"
            | "rb"
            | "kt"
            | "swift"
            | "php"
            | "cs"
    )
}

// ---------------------------------------------------------------- command

fn relative_to_root(root: &Path, raw: &str) -> Result<String> {
    let path = Path::new(raw);
    let relative: PathBuf = if path.is_absolute() {
        let stripped = path
            .strip_prefix(root)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| {
                let parent = path.parent()?.canonicalize().ok()?;
                let rel = parent.strip_prefix(root).ok()?;
                Some(rel.join(path.file_name()?))
            });
        stripped.with_context(|| format!("path is outside the repo root: {raw}"))?
    } else {
        path.to_path_buf()
    };
    let mut parts: Vec<String> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    bail!("path is outside the repo root: {raw}");
                }
            }
            _ => bail!("path is outside the repo root: {raw}"),
        }
    }
    if parts.is_empty() {
        bail!("path is empty: {raw}");
    }
    Ok(parts.join("/"))
}

pub fn run_blast_json(command: &BlastCommand) -> Result<BlastReport> {
    if !command.json {
        bail!("blast currently supports only --json");
    }
    if is_github_repo_link(&command.repo_path) {
        bail!("blast works on local repositories only; clone the repo and pass its path");
    }
    crate::cli::validate_repo_input(&command.repo_path)?;
    let root = command.repo_path.canonicalize()?;
    ensure_git_repo(&root)?;
    let model = scan_repo(&root, command.max_file_bytes, &command.ignore)?;
    let mut changed = git_changed_files(&root)?;
    let mut seen: HashSet<String> = changed.iter().map(|c| c.path.clone()).collect();
    for raw in &command.path {
        let rel = relative_to_root(&root, raw)?;
        if seen.insert(rel.clone()) {
            changed.push(ChangedFile {
                path: rel,
                status: "proposed".to_string(),
            });
        }
    }
    changed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(compute_blast(&model, &changed, created_at_rfc3339()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn js_relative_and_index_resolution() {
        let p = paths(&["src/a.ts", "src/lib/index.tsx", "src/b.ts"]);
        assert_eq!(import_targets("src/b.ts", "./a", &p), vec!["src/a.ts"]);
        assert_eq!(
            import_targets("src/b.ts", "./lib", &p),
            vec!["src/lib/index.tsx"]
        );
        assert_eq!(import_targets("src/b.ts", "./a.js", &p), vec!["src/a.ts"]);
        assert_eq!(
            import_targets("src/b.ts", "@/lib", &p),
            vec!["src/lib/index.tsx"]
        );
        assert!(import_targets("src/b.ts", "react", &p).is_empty());
        assert!(import_targets("src/b.ts", "../../x", &p).is_empty());
    }

    #[test]
    fn python_and_rust_resolution() {
        let p = paths(&[
            "pkg/core.py",
            "pkg/sub/__init__.py",
            "src/repo.rs",
            "src/ui/mod.rs",
            "src/ui/panel.rs",
            "tests/x.rs",
        ]);
        assert_eq!(
            import_targets("app.py", "pkg.core", &p),
            vec!["pkg/core.py"]
        );
        assert_eq!(
            import_targets("app.py", "pkg.sub", &p),
            vec!["pkg/sub/__init__.py"]
        );
        assert_eq!(import_targets("pkg/x.py", ".core", &p), vec!["pkg/core.py"]);
        assert_eq!(
            import_targets("tests/x.rs", "mycrate::repo", &p),
            vec!["src/repo.rs"]
        );
        assert_eq!(
            import_targets("src/ui/mod.rs", "self::panel::Panel", &p),
            vec!["src/ui/panel.rs"]
        );
        assert_eq!(
            import_targets("src/ui/panel.rs", "super::repo", &p).len(),
            0
        );
        assert_eq!(
            import_targets("src/ui/panel.rs", "crate::repo::scan_repo", &p),
            vec!["src/repo.rs"]
        );
        assert!(import_targets("src/ui/panel.rs", "std::fs", &p).is_empty());
    }

    #[test]
    fn stems_and_sensitivity() {
        assert_eq!(test_stem("src/foo.test.ts"), "foo");
        assert_eq!(test_stem("a/test_foo.py"), "foo");
        assert_eq!(test_stem("a/foo_test.go"), "foo");
        assert_eq!(test_stem("tests/foo.rs"), "foo");
        let none = HashSet::new();
        assert!(is_sensitive("Cargo.toml", &none));
        assert!(is_sensitive("src/Auth/login.ts", &none));
        assert!(is_sensitive(".github/workflows/ci.yml", &none));
        assert!(!is_sensitive("src/repo.rs", &none));
    }

    #[test]
    fn relative_path_handling() {
        let root = Path::new("/tmp/does-not-matter");
        assert_eq!(relative_to_root(root, "./src/a.rs").unwrap(), "src/a.rs");
        assert_eq!(
            relative_to_root(root, "/tmp/does-not-matter/src/a.rs").unwrap(),
            "src/a.rs"
        );
        assert!(relative_to_root(root, "../x").is_err());
        assert!(relative_to_root(root, "/etc/passwd").is_err());
    }
}
