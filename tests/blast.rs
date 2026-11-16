use std::{fs, path::Path, process::Command};

use branch_futures::{
    blast::{run_blast_json, BlastReport},
    cli::BlastCommand,
};
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn repo(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    for (rel, body) in files {
        write(dir.path(), rel, body);
    }
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn blast(root: &Path, paths: &[&str]) -> BlastReport {
    run_blast_json(&BlastCommand {
        json: true,
        repo_path: root.to_path_buf(),
        path: paths.iter().map(|p| p.to_string()).collect(),
        max_file_bytes: 200_000,
        ignore: Vec::new(),
    })
    .unwrap()
}

#[test]
fn js_chain_depths_and_tests() {
    let dir = repo(&[
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from './a';\nexport const b = a;\n",
        ),
        (
            "src/c.ts",
            "import { b } from './b';\nexport const c = b;\n",
        ),
        ("src/a.test.ts", "import { a } from './a';\n"),
    ]);
    write(dir.path(), "src/a.ts", "export const a = 2;\n");
    let report = blast(dir.path(), &[]);

    assert_eq!(report.kind, "blast");
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].path, "src/a.ts");
    assert_eq!(report.changed[0].status, "modified");
    let b = report
        .impacted
        .iter()
        .find(|i| i.path == "src/b.ts")
        .unwrap();
    assert_eq!(b.depth, 1);
    assert_eq!(b.via, vec!["src/a.ts"]);
    let c = report
        .impacted
        .iter()
        .find(|i| i.path == "src/c.ts")
        .unwrap();
    assert_eq!(c.depth, 2);
    assert_eq!(c.via, vec!["src/b.ts"]);
    assert!(report.tests.contains(&"src/a.test.ts".to_string()));
    assert!(report.untested_changes.is_empty());
    assert!(report.impacted.iter().all(|i| i.path != "src/a.ts"));
}

#[test]
fn python_dotted_import_is_resolved() {
    let dir = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/core.py", "def x():\n    return 1\n"),
        ("app.py", "from pkg.core import x\n"),
    ]);
    write(dir.path(), "pkg/core.py", "def x():\n    return 2\n");
    let report = blast(dir.path(), &[]);
    let app = report.impacted.iter().find(|i| i.path == "app.py").unwrap();
    assert_eq!(app.depth, 1);
    assert_eq!(app.via, vec!["pkg/core.py"]);
    assert_eq!(report.untested_changes, vec!["pkg/core.py"]);
}

#[test]
fn rust_integration_test_resolves_to_src_module() {
    let dir = repo(&[
        ("Cargo.toml", "[package]\nname = \"mycrate\"\n"),
        ("src/lib.rs", "pub mod repo;\n"),
        ("src/repo.rs", "pub fn scan() {}\n"),
        ("tests/x.rs", "use mycrate::repo;\n"),
    ]);
    write(dir.path(), "src/repo.rs", "pub fn scan() { }\n");
    let report = blast(dir.path(), &[]);
    let test = report
        .impacted
        .iter()
        .find(|i| i.path == "tests/x.rs")
        .unwrap();
    assert_eq!(test.depth, 1);
    assert!(test.is_test);
    assert!(report.tests.contains(&"tests/x.rs".to_string()));
}

#[test]
fn path_flag_adds_proposed_files_and_rejects_outside() {
    let dir = repo(&[
        ("src/a.ts", "export const a = 1;\n"),
        ("src/b.ts", "import { a } from './a';\n"),
    ]);
    let report = blast(dir.path(), &["src/a.ts"]);
    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].status, "proposed");
    assert_eq!(report.impacted.len(), 1);

    let abs = dir.path().canonicalize().unwrap().join("src/a.ts");
    let report = blast(dir.path(), &[abs.to_str().unwrap()]);
    assert_eq!(report.changed[0].path, "src/a.ts");

    let err = run_blast_json(&BlastCommand {
        json: true,
        repo_path: dir.path().to_path_buf(),
        path: vec!["../escape.ts".to_string()],
        max_file_bytes: 200_000,
        ignore: Vec::new(),
    });
    assert!(err.is_err());
}

#[test]
fn existing_git_change_wins_over_proposed_and_untracked_is_reported() {
    let dir = repo(&[("src/a.ts", "export const a = 1;\n")]);
    write(dir.path(), "src/a.ts", "export const a = 3;\n");
    write(dir.path(), "src/new.ts", "export const n = 1;\n");
    let report = blast(dir.path(), &["src/a.ts"]);
    let status = |p: &str| {
        report
            .changed
            .iter()
            .find(|c| c.path == p)
            .unwrap()
            .status
            .clone()
    };
    assert_eq!(status("src/a.ts"), "modified");
    assert_eq!(status("src/new.ts"), "untracked");
}

#[test]
fn empty_diff_scores_zero_low() {
    let dir = repo(&[("src/a.ts", "export const a = 1;\n")]);
    let report = blast(dir.path(), &[]);
    assert_eq!(report.score, 0);
    assert_eq!(report.level, "low");
    assert_eq!(report.reasons, vec!["no working-tree changes"]);
    assert!(report.changed.is_empty());
}

#[test]
fn sensitive_path_raises_level() {
    let dir = repo(&[
        ("src/auth/session.ts", "export const s = 1;\n"),
        ("src/login.ts", "import { s } from './auth/session';\n"),
        ("package.json", "{}\n"),
        ("Cargo.toml", "[package]\nname=\"x\"\n"),
    ]);
    write(dir.path(), "src/auth/session.ts", "export const s = 2;\n");
    write(dir.path(), "package.json", "{\"a\":1}\n");
    write(dir.path(), "Cargo.toml", "[package]\nname=\"y\"\n");
    let report = blast(dir.path(), &[]);
    assert_eq!(report.sensitive.len(), 3);
    assert!(report.score >= 7, "score {}", report.score);
    assert_eq!(report.level, "high");
    assert!(report.reasons.iter().any(|r| r.contains("sensitive path")));
}

#[test]
fn github_urls_are_rejected() {
    let err = run_blast_json(&BlastCommand {
        json: true,
        repo_path: "https://github.com/owner/repo".into(),
        path: Vec::new(),
        max_file_bytes: 200_000,
        ignore: Vec::new(),
    })
    .unwrap_err();
    assert!(err.to_string().contains("local"));
}

#[test]
fn subdirectory_root_reports_paths_relative_and_excludes_outside() {
    let dir = repo(&[
        ("pkg/src/util.ts", "export const u = 1;\n"),
        ("pkg/src/a.ts", "import { u } from './util';\n"),
        ("other/o.ts", "export const o = 1;\n"),
    ]);
    write(dir.path(), "pkg/src/util.ts", "export const u = 2;\n");
    write(dir.path(), "other/o.ts", "export const o = 2;\n");
    write(dir.path(), "other/new.ts", "export const n = 1;\n");
    write(dir.path(), "pkg/src/fresh.ts", "export const f = 1;\n");
    let report = blast(&dir.path().join("pkg"), &[]);
    let mut changed: Vec<(&str, &str)> = report
        .changed
        .iter()
        .map(|c| (c.path.as_str(), c.status.as_str()))
        .collect();
    changed.sort();
    assert_eq!(
        changed,
        vec![("src/fresh.ts", "untracked"), ("src/util.ts", "modified")]
    );
    assert_eq!(report.impacted.len(), 1);
    assert_eq!(report.impacted[0].path, "src/a.ts");
}

#[test]
fn subdirectory_fingerprint_ignores_outside_changes() {
    let dir = repo(&[
        ("pkg/a.ts", "export const a = 1;\n"),
        ("o.ts", "export {};\n"),
    ]);
    let before = branch_futures::blast::diff_fingerprint(&dir.path().join("pkg")).unwrap();
    write(dir.path(), "o.ts", "export const z = 1;\n");
    write(dir.path(), "outside_new.ts", "export {};\n");
    let after = branch_futures::blast::diff_fingerprint(&dir.path().join("pkg")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn quoted_special_paths_are_reported_verbatim() {
    let names = ["q\"uote.ts", "tab\there.ts", "back\\slash.ts", "ünï.ts"];
    let dir = repo(&[("base.ts", "export {};\n")]);
    for n in names {
        write(dir.path(), n, "export const x = 1;\n");
    }
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "two"]);
    for n in names {
        write(dir.path(), n, "export const x = 2;\n");
    }
    write(dir.path(), "new\"q.ts", "export {};\n");
    let report = blast(dir.path(), &[]);
    let got: Vec<&str> = report.changed.iter().map(|c| c.path.as_str()).collect();
    for n in names.iter().chain(["new\"q.ts"].iter()) {
        assert!(got.contains(n), "missing {n:?} in {got:?}");
    }
    assert!(got.iter().all(|p| !p.starts_with('"')), "{got:?}");
}

#[test]
fn staged_rename_impacts_importers_of_old_path() {
    let dir = repo(&[
        ("src/old.ts", "export const o = 1;\n"),
        ("src/q.ts", "import { o } from './old';\n"),
    ]);
    git(dir.path(), &["mv", "src/old.ts", "src/new.ts"]);
    let report = blast(dir.path(), &[]);
    let status = |p: &str| {
        report
            .changed
            .iter()
            .find(|c| c.path == p)
            .map(|c| c.status.clone())
    };
    assert_eq!(status("src/old.ts").as_deref(), Some("deleted"));
    assert!(status("src/new.ts").is_some());
    let q = report
        .impacted
        .iter()
        .find(|i| i.path == "src/q.ts")
        .unwrap();
    assert_eq!(q.via, vec!["src/old.ts"]);
}

#[test]
fn rust_mod_declaration_and_use_groups_are_dependencies() {
    let dir = repo(&[
        ("Cargo.toml", "[package]\nname = \"c\"\n"),
        (
            "src/lib.rs",
            "mod child;\nuse crate::{shared::x, other::y};\n",
        ),
        ("src/child.rs", "pub fn c() {}\n"),
        ("src/shared.rs", "pub fn x() {}\n"),
        ("src/other.rs", "pub fn y() {}\n"),
    ]);
    for f in ["child", "shared", "other"] {
        write(dir.path(), &format!("src/{f}.rs"), "pub fn c() { }\n");
    }
    let report = blast(dir.path(), &[]);
    let lib = report
        .impacted
        .iter()
        .find(|i| i.path == "src/lib.rs")
        .unwrap();
    assert_eq!(
        lib.via,
        vec!["src/child.rs", "src/other.rs", "src/shared.rs"]
    );
}

#[test]
fn python_relative_and_package_member_imports_are_dependencies() {
    let dir = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/util.py", "def u(): pass\n"),
        ("pkg/mod.py", "def m(): pass\n"),
        ("pkg/a.py", "from . import util\n"),
        ("pkg/b.py", "from .mod import m\n"),
        ("top.py", "from pkg import util\n"),
        ("multi.py", "import os, pkg.mod as pm\n"),
    ]);
    write(dir.path(), "pkg/util.py", "def u(): return 1\n");
    write(dir.path(), "pkg/mod.py", "def m(): return 1\n");
    let report = blast(dir.path(), &[]);
    let via = |p: &str| -> Vec<String> {
        report
            .impacted
            .iter()
            .find(|i| i.path == p)
            .unwrap_or_else(|| panic!("{p} not impacted: {:?}", report.impacted))
            .via
            .clone()
    };
    assert_eq!(via("pkg/a.py"), vec!["pkg/util.py"]);
    assert_eq!(via("pkg/b.py"), vec!["pkg/mod.py"]);
    assert!(via("top.py").contains(&"pkg/util.py".to_string()));
    assert_eq!(via("multi.py"), vec!["pkg/mod.py"]);
}

#[test]
fn js_reexport_and_dynamic_import_are_dependencies() {
    let dir = repo(&[
        ("src/x.ts", "export const x = 1;\n"),
        ("src/y.ts", "export const y = 1;\n"),
        ("src/z.ts", "export const z = 1;\n"),
        (
            "src/index.ts",
            "export * from './x';\nexport { y } from './y';\nconst z = import('./z');\n",
        ),
    ]);
    for f in ["x", "y", "z"] {
        write(dir.path(), &format!("src/{f}.ts"), "export const q = 2;\n");
    }
    let report = blast(dir.path(), &[]);
    let idx = report
        .impacted
        .iter()
        .find(|i| i.path == "src/index.ts")
        .unwrap();
    assert_eq!(idx.via, vec!["src/x.ts", "src/y.ts", "src/z.ts"]);
}

#[test]
fn late_alphabetical_import_is_in_graph() {
    let mut body = String::new();
    for i in 0..40 {
        body.push_str(&format!("import './m{i:02}';\n"));
    }
    let mut files: Vec<(String, String)> = (0..40)
        .map(|i| (format!("src/m{i:02}.ts"), "export {};\n".to_string()))
        .collect();
    files.push(("src/big.ts".to_string(), body));
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let dir = repo(&refs);
    write(dir.path(), "src/m39.ts", "export const k = 1;\n");
    let report = blast(dir.path(), &[]);
    assert!(report.impacted.iter().any(|i| i.path == "src/big.ts"));
}

#[test]
fn sensitivity_uses_word_boundaries() {
    let dir = repo(&[
        ("src/author.ts", "export {};\n"),
        ("src/tokenizer.ts", "export {};\n"),
        ("src/auth.ts", "export {};\n"),
        ("src/user_auth.py", "x = 1\n"),
        ("src/session_store.rs", "fn f() {}\n"),
        ("src/userAuth.ts", "export {};\n"),
        ("src/migrations/001.sql", "select 1;\n"),
        ("src/authentication.ts", "export {};\n"),
    ]);
    for p in [
        "src/author.ts",
        "src/tokenizer.ts",
        "src/auth.ts",
        "src/user_auth.py",
        "src/session_store.rs",
        "src/userAuth.ts",
        "src/migrations/001.sql",
        "src/authentication.ts",
    ] {
        write(dir.path(), p, "// changed\n");
    }
    let report = blast(dir.path(), &[]);
    let mut got = report.sensitive.clone();
    got.sort();
    assert_eq!(
        got,
        vec![
            "src/auth.ts",
            "src/authentication.ts",
            "src/migrations/001.sql",
            "src/session_store.rs",
            "src/userAuth.ts",
            "src/user_auth.py",
        ]
    );
}

#[test]
fn non_git_directory_fails_with_clear_message() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.ts", "export {};\n");
    let err = run_blast_json(&BlastCommand {
        json: true,
        repo_path: dir.path().to_path_buf(),
        path: Vec::new(),
        max_file_bytes: 200_000,
        ignore: Vec::new(),
    })
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.starts_with("not a git repository: "), "{msg}");
    assert!(!msg.contains("usage:"), "{msg}");
}
