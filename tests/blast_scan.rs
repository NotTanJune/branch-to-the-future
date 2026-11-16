use std::{fs, path::Path};

use branch_futures::repo::scan_repo;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn scanned(files: &[&str], ignore: &[&str]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        write(dir.path(), f, "export const x = 1;\n");
    }
    let ignore: Vec<String> = ignore.iter().map(|s| s.to_string()).collect();
    let model = scan_repo(dir.path(), 200_000, &ignore).unwrap();
    model.files.into_iter().map(|f| f.path).collect()
}

#[test]
fn default_ignores_match_whole_segments_only() {
    let kept = [
        "src/builder.ts",
        "tests/distance.test.ts",
        "build.rs",
        "targets.rs",
        ".github/workflows/ci.yml",
        ".gitignore",
    ];
    let dropped = [
        "build/out.js",
        "dist/a.js",
        "target/x.rs",
        "node_modules/p/i.js",
        "venv/lib.py",
        "src/build/x.ts",
        ".env",
        ".env.local",
        ".env.example",
    ];
    let mut all: Vec<&str> = kept.to_vec();
    all.extend(dropped);
    let got = scanned(&all, &[]);
    for k in kept {
        assert!(got.iter().any(|p| p == k), "{k} should be kept: {got:?}");
    }
    for d in dropped {
        assert!(
            !got.iter().any(|p| p == d),
            "{d} should be ignored: {got:?}"
        );
    }
}

#[test]
fn workflows_are_config() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".github/workflows/ci.yml", "on: push\n");
    let model = scan_repo(dir.path(), 200_000, &[]).unwrap();
    assert!(model
        .config_files
        .contains(&".github/workflows/ci.yml".to_string()));
}

#[test]
fn user_ignore_uses_gitignore_glob_semantics() {
    let files = [
        "src/a.ts",
        "src/a/inner.ts",
        "src/ab.ts",
        "lib/src/a.ts",
        "src/latest.ts",
        "test/x.ts",
        "src/test/y.ts",
        "notes.md",
    ];
    let got = scanned(&files, &["src/a"]);
    assert!(got.contains(&"src/a.ts".to_string()), "{got:?}");
    assert!(got.contains(&"src/ab.ts".to_string()), "{got:?}");
    assert!(got.contains(&"lib/src/a.ts".to_string()), "{got:?}");
    assert!(!got.contains(&"src/a/inner.ts".to_string()), "{got:?}");

    let got = scanned(&files, &["test"]);
    assert!(got.contains(&"src/latest.ts".to_string()), "{got:?}");
    assert!(!got.contains(&"test/x.ts".to_string()), "{got:?}");
    assert!(!got.contains(&"src/test/y.ts".to_string()), "{got:?}");

    let got = scanned(&files, &["*.ts"]);
    assert_eq!(got, vec!["notes.md".to_string()], "{got:?}");

    let got = scanned(&files, &["src/a.ts"]);
    assert!(!got.contains(&"src/a.ts".to_string()), "{got:?}");
    assert!(got.contains(&"lib/src/a.ts".to_string()), "{got:?}");

    let got = scanned(&files, &["/src/"]);
    assert!(!got.iter().any(|p| p.starts_with("src/")), "{got:?}");
    assert!(got.contains(&"lib/src/a.ts".to_string()), "{got:?}");
}

#[test]
fn imports_are_not_capped_at_32() {
    let mut body = String::new();
    for i in 0..45 {
        body.push_str(&format!("import './m{i:02}';\n"));
    }
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/big.ts", &body);
    let model = scan_repo(dir.path(), 200_000, &[]).unwrap();
    let big = model.files.iter().find(|f| f.path == "src/big.ts").unwrap();
    assert_eq!(big.imports.len(), 45);
    assert!(big.imports.contains(&"./m44".to_string()));
}
