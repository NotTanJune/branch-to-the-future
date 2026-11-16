use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Context, Result};
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    WalkBuilder,
};
use regex::Regex;

use crate::domain::{FileKind, RepoFile, RepoModel, RiskSignal, RouteInfo};

const DEFAULT_IGNORES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".env",
];

pub fn scan_repo(
    root: &Path,
    max_file_bytes: u64,
    ignore_patterns: &[String],
) -> Result<RepoModel> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let mut files = Vec::new();
    let mut routes = Vec::new();
    let mut tests = Vec::new();
    let mut config_files = Vec::new();
    let mut risk_signals = Vec::new();
    let mut framework_files = Vec::new();

    let user_ignore = build_user_ignore(&root, ignore_patterns);
    let mut walker = WalkBuilder::new(&root);
    walker.standard_filters(true).hidden(false).git_ignore(true);
    for pattern in DEFAULT_IGNORES {
        walker.add_ignore(format!("{pattern}/"));
    }

    for result in walker.build() {
        let entry = result?;
        if !entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path();
        let rel = rel_path(&root, path)?;
        if should_ignore(&rel, &user_ignore) || is_binary_like(path) {
            continue;
        }

        let metadata = entry.metadata()?;
        if metadata.len() > max_file_bytes {
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let kind = classify(&rel);
        let symbols = extract_symbols(&rel, &content);
        let imports = extract_imports(&rel, &content);
        let snippets = extract_snippets(&content, &symbols, &imports);

        if kind == FileKind::Test {
            tests.push(rel.clone());
        }
        if kind == FileKind::Config {
            config_files.push(rel.clone());
        }
        if let Some(route) = route_info(&rel, &content) {
            routes.push(route);
        }
        if let Some(signal) = risk_signal(&rel, &content) {
            risk_signals.push(signal);
        }
        if is_framework_file(&rel) {
            framework_files.push((rel.clone(), content.clone()));
        }

        files.push(RepoFile {
            path: rel,
            kind,
            size: metadata.len() as usize,
            symbols,
            imports,
            snippets,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    tests.sort();
    config_files.sort();
    routes.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(RepoModel {
        repo_name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
            .to_string(),
        root_path: root.display().to_string(),
        frameworks: detect_frameworks(&root, &framework_files),
        files,
        routes,
        tests,
        config_files,
        risk_signals,
    })
}

fn rel_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Compiles user `--ignore` patterns with gitignore semantics, rooted at the scan root.
fn build_user_ignore(root: &Path, patterns: &[String]) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        // An invalid glob is skipped rather than aborting the scan.
        let _ = builder.add_line(None, pattern);
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn should_ignore(path: &str, user_ignore: &Gitignore) -> bool {
    let default_hit = path
        .split('/')
        .any(|segment| DEFAULT_IGNORES.contains(&segment) || segment.starts_with(".env."));
    default_hit
        || user_ignore
            .matched_path_or_any_parents(Path::new(path), false)
            .is_ignore()
}

fn is_binary_like(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "pdf"
            | "zip"
            | "gz"
            | "tar"
            | "mp4"
            | "mov"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
            | "wasm"
            | "lockb"
    )
}

fn classify(path: &str) -> FileKind {
    let lower = path.to_ascii_lowercase();
    let name = PathBuf::from(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if is_config(path) {
        return FileKind::Config;
    }
    if lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.starts_with("__tests__/")
        || lower.contains("/__tests__/")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with("_test.py")
        || lower.ends_with(".test.rs")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_tests.rs")
    {
        return FileKind::Test;
    }
    if lower.contains("/db/")
        || lower.contains("/schema/")
        || name == "schema.sql"
        || path.ends_with(".prisma")
        || path.ends_with(".sql")
    {
        return FileKind::Schema;
    }
    if is_route(path) {
        return FileKind::Route;
    }
    if lower.contains("/workers/") || lower.contains("/jobs/") || lower.contains("/queue/") {
        return FileKind::Worker;
    }
    if lower.contains("/services/") {
        return FileKind::Service;
    }
    if lower.contains("/controllers/") {
        return FileKind::Controller;
    }
    if lower.contains("/components/") || lower.ends_with(".tsx") || lower.ends_with(".jsx") {
        return FileKind::UiComponent;
    }
    if lower.ends_with(".ts") || lower.ends_with(".tsx") {
        return FileKind::TypeScript;
    }
    if lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".mjs")
        || lower.ends_with(".cjs")
    {
        return FileKind::JavaScript;
    }
    if lower.ends_with(".py") {
        return FileKind::Python;
    }
    if lower.ends_with(".rs") {
        return FileKind::Rust;
    }
    FileKind::Unknown
}

fn is_config(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "package.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "package-lock.json"
            | "tsconfig.json"
            | "vite.config.ts"
            | "vite.config.js"
            | "next.config.ts"
            | "next.config.js"
            | "next.config.mjs"
            | "pyproject.toml"
            | "requirements.txt"
            | "cargo.toml"
            | ".env.example"
    ) || lower.starts_with(".github/workflows/")
}

fn is_route(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/api/")
        || lower.starts_with("api/")
        || lower.starts_with("pages/api/")
        || lower.contains("/routes/")
        || lower.ends_with("/route.ts")
        || lower.ends_with("/route.js")
}

fn extract_imports(path: &str, content: &str) -> Vec<String> {
    let mut imports = BTreeSet::new();
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".py") {
        python_imports(content, &mut imports);
    } else if lower.ends_with(".rs") {
        rust_imports(content, &mut imports);
    } else {
        js_imports(content, &mut imports);
    }
    imports.remove("");
    imports.into_iter().collect()
}

fn js_imports(content: &str, out: &mut BTreeSet<String>) {
    static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
            r#"(?m)^\s*import\s+(?:[^'"]+\s+from\s+)?['"]([^'"]+)['"]"#,
            r#"(?m)^\s*export\s+(?:type\s+)?(?:\*(?:\s+as\s+[\w$]+)?|\{[^}]*\})\s+from\s+['"]([^'"]+)['"]"#,
            r#"\brequire\(\s*['"]([^'"]+)['"]\s*\)"#,
            r#"\bimport\(\s*['"]([^'"]+)['"]\s*\)"#,
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    });
    for re in PATTERNS.iter() {
        for capture in re.captures_iter(content) {
            if let Some(value) = capture.get(1) {
                out.insert(value.as_str().to_string());
            }
        }
    }
}

fn python_imports(content: &str, out: &mut BTreeSet<String>) {
    static FROM: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r#"(?m)^[ \t]*from[ \t]+([A-Za-z0-9_\.]+)[ \t]+import[ \t]*(?:\(([^)]*)\)|([^\n]*))"#,
        )
        .ok()
    });
    static IMPORT: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r#"(?m)^[ \t]*import[ \t]+([^\n;]+)"#).ok());

    if let Some(re) = FROM.as_ref() {
        for capture in re.captures_iter(content) {
            let module = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
            out.insert(module.to_string());
            let names = capture
                .get(2)
                .or_else(|| capture.get(3))
                .map(|m| m.as_str())
                .unwrap_or_default();
            for line in names.lines() {
                let line = line.split('#').next().unwrap_or_default();
                for item in line.split(',') {
                    let name = item.split_whitespace().next().unwrap_or_default();
                    let name = name.trim_matches(|c: char| c == '(' || c == ')' || c == '\\');
                    if name.is_empty()
                        || name == "*"
                        || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        continue;
                    }
                    if module.ends_with('.') {
                        out.insert(format!("{module}{name}"));
                    } else {
                        out.insert(format!("{module}.{name}"));
                    }
                }
            }
        }
    }
    if let Some(re) = IMPORT.as_ref() {
        for capture in re.captures_iter(content) {
            let list = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
            let list = list.split('#').next().unwrap_or_default();
            for item in list.split(',') {
                let name = item.split_whitespace().next().unwrap_or_default();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                {
                    out.insert(name.to_string());
                }
            }
        }
    }
}

fn rust_imports(content: &str, out: &mut BTreeSet<String>) {
    static USE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r#"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?use[ \t]+([^;]+);"#).ok()
    });
    static MOD: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r#"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*;"#,
        )
        .ok()
    });
    static ALIAS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s+as\s+[A-Za-z_][A-Za-z0-9_]*").expect("valid regex"));
    if let Some(re) = USE.as_ref() {
        for capture in re.captures_iter(content) {
            let without_alias = ALIAS.replace_all(&capture[1], "");
            let tree: String = without_alias
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            expand_use_tree("", &tree, out);
        }
    }
    if let Some(re) = MOD.as_ref() {
        for capture in re.captures_iter(content) {
            // Resolved relative to the declaring file's module directory.
            out.insert(format!("self::{}", &capture[1]));
        }
    }
}

/// Expands a whitespace-free `use` tree into full paths, one per leaf.
fn expand_use_tree(prefix: &str, tree: &str, out: &mut BTreeSet<String>) {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut parts = Vec::new();
    for (idx, ch) in tree.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&tree[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&tree[start..]);
    for part in parts {
        let part = part.trim_start_matches("::");
        if part.is_empty() {
            continue;
        }
        if let Some(brace) = part.find('{') {
            let head = part[..brace].trim_end_matches("::");
            let inner = part[brace + 1..]
                .strip_suffix('}')
                .unwrap_or(&part[brace + 1..]);
            let full = join_use(prefix, head);
            if full.matches("::").count() >= 1 {
                out.insert(full.clone());
            }
            expand_use_tree(&full, inner, out);
            continue;
        }
        let leaf = part.trim_end_matches('*').trim_end_matches("::");
        let full = if leaf == "self" {
            prefix.to_string()
        } else {
            join_use(prefix, leaf)
        };
        if !full.is_empty() {
            out.insert(full);
        }
    }
}

fn join_use(prefix: &str, rest: &str) -> String {
    match (prefix.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (_, true) => prefix.to_string(),
        _ => format!("{prefix}::{rest}"),
    }
}

fn extract_symbols(path: &str, content: &str) -> Vec<String> {
    let mut symbols = BTreeSet::new();
    let lower = path.to_ascii_lowercase();
    let patterns = if lower.ends_with(".py") {
        vec![
            r#"(?m)^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            r#"(?m)^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)"#,
        ]
    } else if lower.ends_with(".rs") {
        vec![
            r#"(?m)\bfn\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            r#"(?m)\bstruct\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            r#"(?m)\benum\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            r#"(?m)\btrait\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            r#"(?m)\bimpl\s+([A-Za-z_][A-Za-z0-9_]*)"#,
        ]
    } else {
        vec![
            r#"(?m)\b(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)"#,
            r#"(?m)\b(?:export\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)"#,
            r#"(?m)\b(?:export\s+)?const\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*="#,
        ]
    };

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for capture in re.captures_iter(content) {
                if let Some(value) = capture.get(1) {
                    symbols.insert(value.as_str().to_string());
                }
            }
        }
    }
    symbols.into_iter().take(48).collect()
}

fn extract_snippets(content: &str, symbols: &[String], imports: &[String]) -> Vec<String> {
    let mut snippets = Vec::new();
    let needles: Vec<&str> = symbols
        .iter()
        .chain(imports.iter())
        .map(String::as_str)
        .collect();
    for line in content.lines() {
        if snippets.len() >= 8 {
            break;
        }
        if needles
            .iter()
            .any(|needle| !needle.is_empty() && line.contains(needle))
        {
            let compact = line.trim();
            if !compact.is_empty() {
                snippets.push(compact.chars().take(220).collect());
            }
        }
    }
    snippets
}

fn route_info(path: &str, content: &str) -> Option<RouteInfo> {
    if !is_route(path) {
        return None;
    }
    let method_re = Regex::new(r#"\b(GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD)\b"#).ok()?;
    let method = method_re
        .captures(content)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "ANY".to_string());
    Some(RouteInfo {
        path: path.to_string(),
        method,
        route: infer_route(path),
    })
}

fn infer_route(path: &str) -> String {
    let mut route = path.to_string();
    for prefix in ["src/app/", "app/", "src/pages/", "pages/"] {
        if let Some(stripped) = route.strip_prefix(prefix) {
            route = stripped.to_string();
            break;
        }
    }
    for suffix in ["/route.ts", "/route.js", ".ts", ".tsx", ".js", ".jsx"] {
        if let Some(stripped) = route.strip_suffix(suffix) {
            route = stripped.to_string();
            break;
        }
    }
    route = route.replace("/index", "");
    if !route.starts_with('/') {
        route.insert(0, '/');
    }
    route
}

fn risk_signal(path: &str, content: &str) -> Option<RiskSignal> {
    let lower_path = path.to_ascii_lowercase();
    let lower_content = content.to_ascii_lowercase();
    if lower_path.contains("upload") && !lower_content.contains("validate") {
        return Some(RiskSignal {
            path: path.to_string(),
            signal: "file upload without visible validation".to_string(),
        });
    }
    if lower_content.contains("process.env") {
        return Some(RiskSignal {
            path: path.to_string(),
            signal: "environment-dependent behavior".to_string(),
        });
    }
    None
}

fn is_framework_file(path: &str) -> bool {
    matches!(
        path,
        "package.json"
            | "next.config.ts"
            | "next.config.js"
            | "next.config.mjs"
            | "vite.config.ts"
            | "vite.config.js"
            | "pyproject.toml"
            | "requirements.txt"
            | "Cargo.toml"
    )
}

fn detect_frameworks(root: &Path, files: &[(String, String)]) -> Vec<String> {
    let mut frameworks = BTreeSet::new();
    for (path, content) in files {
        let lower = path.to_ascii_lowercase();
        if lower == "package.json" {
            if content.contains("\"next\"") {
                frameworks.insert("Next.js".to_string());
            }
            if content.contains("\"react\"") {
                frameworks.insert("React".to_string());
            }
            if content.contains("\"typescript\"") {
                frameworks.insert("TypeScript".to_string());
            }
            if content.contains("\"express\"") {
                frameworks.insert("Express".to_string());
            }
            if content.contains("\"@nestjs") {
                frameworks.insert("NestJS".to_string());
            }
        }
        if lower.starts_with("next.config.") {
            frameworks.insert("Next.js".to_string());
        }
        if lower.starts_with("vite.config.") {
            frameworks.insert("Vite".to_string());
        }
        if lower == "pyproject.toml" || lower == "requirements.txt" {
            frameworks.insert("Python".to_string());
            if content.to_ascii_lowercase().contains("fastapi") {
                frameworks.insert("FastAPI".to_string());
            }
            if content.to_ascii_lowercase().contains("django") {
                frameworks.insert("Django".to_string());
            }
        }
        if lower == "cargo.toml" {
            frameworks.insert("Rust".to_string());
        }
    }
    if root.join("app").exists() && root.join("package.json").exists() {
        frameworks.insert("Next.js".to_string());
    }
    frameworks.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imps(path: &str, content: &str) -> Vec<String> {
        extract_imports(path, content)
    }

    fn has(list: &[String], item: &str) -> bool {
        list.iter().any(|i| i == item)
    }

    #[test]
    fn env_example_is_config_when_not_scanned_out() {
        assert!(is_config(".env.example"));
        assert!(is_config(".github/workflows/ci.yml"));
    }

    #[test]
    fn rust_mod_and_brace_groups() {
        let got = imps(
            "src/lib.rs",
            "mod x;\npub mod y;\nmod inline { }\nuse crate::{a::x, b::y};\nuse crate::c::{z};\nuse crate::d::{self, e as f};\nuse std::{fs, io};\nuse crate::g::*;\nuse crate::h::{\n    i,\n    j::k,\n};\n",
        );
        for want in [
            "self::x",
            "self::y",
            "crate::a::x",
            "crate::b::y",
            "crate::c",
            "crate::c::z",
            "crate::d",
            "crate::d::e",
            "std::fs",
            "crate::g",
            "crate::h::i",
            "crate::h::j::k",
        ] {
            assert!(has(&got, want), "missing {want}: {got:?}");
        }
        assert!(!has(&got, "self::inline"), "{got:?}");
        assert!(
            !got.iter().any(|i| i.contains('{') || i.contains(' ')),
            "{got:?}"
        );
    }

    #[test]
    fn js_reexports_and_dynamic_import() {
        let got = imps(
            "src/index.ts",
            "export * from \"./x\";\nexport * as ns from './n';\nexport { a, b } from './y';\nexport type { T } from './t';\nconst m = await import('./z');\nimport w from './w';\nconst r = require('./r');\nexport const local = 1;\n",
        );
        for want in ["./x", "./n", "./y", "./t", "./z", "./w", "./r"] {
            assert!(has(&got, want), "missing {want}: {got:?}");
        }
        assert_eq!(got.len(), 7, "{got:?}");
    }

    #[test]
    fn python_relative_from_and_lists() {
        let got = imps(
            "pkg/app.py",
            "from . import util\nfrom .mod import x, y as z\nfrom pkg import helper\nfrom .. import top\nfrom a.b import (\n    c,  # note\n    d,\n)\nimport os, pkg.util as u, json\nfrom q import *\n",
        );
        for want in [
            ".",
            ".util",
            ".mod",
            ".mod.x",
            ".mod.y",
            "pkg",
            "pkg.helper",
            "..",
            "..top",
            "a.b",
            "a.b.c",
            "a.b.d",
            "os",
            "pkg.util",
            "json",
            "q",
        ] {
            assert!(has(&got, want), "missing {want}: {got:?}");
        }
        assert!(!has(&got, "q.*"), "{got:?}");
        assert!(!has(&got, "u"), "{got:?}");
    }

    #[test]
    fn imports_are_not_capped() {
        let body: String = (0..60).map(|i| format!("import m{i:02}\n")).collect();
        let got = imps("a.py", &body);
        assert_eq!(got.len(), 60);
        assert!(has(&got, "m59"));
    }
}
