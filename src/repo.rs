use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
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
        if should_ignore(&rel, ignore_patterns) || is_binary_like(path) {
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

fn should_ignore(path: &str, ignore_patterns: &[String]) -> bool {
    DEFAULT_IGNORES
        .iter()
        .any(|pattern| path == *pattern || path.starts_with(&format!("{pattern}/")))
        || ignore_patterns.iter().any(|pattern| {
            let pattern = pattern.trim_matches('/');
            !pattern.is_empty()
                && (path == pattern
                    || path.starts_with(&format!("{pattern}/"))
                    || path.contains(pattern))
        })
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
    if lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with("_test.py")
        || lower.ends_with(".test.rs")
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
    let patterns = if lower.ends_with(".py") {
        vec![
            r#"(?m)^\s*from\s+([A-Za-z0-9_\.]+)\s+import"#,
            r#"(?m)^\s*import\s+([A-Za-z0-9_\.]+)"#,
        ]
    } else if lower.ends_with(".rs") {
        vec![r#"(?m)^\s*use\s+([A-Za-z0-9_:]+)"#]
    } else {
        vec![
            r#"(?m)^\s*import\s+(?:[^'"]+\s+from\s+)?['"]([^'"]+)['"]"#,
            r#"(?m)require\(\s*['"]([^'"]+)['"]\s*\)"#,
        ]
    };

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for capture in re.captures_iter(content) {
                if let Some(value) = capture.get(1) {
                    imports.insert(value.as_str().trim_end_matches(';').to_string());
                }
            }
        }
    }
    imports.into_iter().take(32).collect()
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
