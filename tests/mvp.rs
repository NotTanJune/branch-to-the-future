use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

use branch_futures::ai::{parse_impact_analysis, AiStreamEvent, OpenAiClient};
use branch_futures::artifacts::export_markdown;
use branch_futures::cli::{resolve_openai_api_key, validate_startup, Cli, ReasoningEffort};
use branch_futures::domain::{
    Complexity, ImpactAnalysis, ImplementationFuture, RepoModel, RiskLevel,
};
use branch_futures::repo::scan_repo;
use branch_futures::repo_source::{
    github_authorization_header, github_clone_error_message, prepare_repo_source_with_cloner,
    resolve_github_token, GitHubRepo,
};
use clap::CommandFactory;

#[test]
fn cli_command_name_is_brf() {
    assert_eq!(Cli::command().get_name(), "brf");
}

#[test]
fn cli_help_names_repo_input_as_path_or_github_url() {
    let mut command = Cli::command();
    let help = command.render_help().to_string();

    assert!(help.contains("REPO_PATH_OR_GITHUB_URL"));
}

#[test]
fn cli_defaults_are_cost_optimized_for_prototyping() {
    let cli = Cli::parse_from(["brf", "."]);

    assert_eq!(cli.text_model, "gpt-5-mini");
    assert_eq!(cli.reasoning_effort, ReasoningEffort::Low);
    assert_eq!(cli.max_output_tokens, 8_000);
    assert_eq!(cli.max_prompt_files, 80);
}

#[test]
fn startup_requires_existing_repo_and_api_key() {
    let missing = Cli::parse_from(["brf", "/definitely/missing"]);
    let err = validate_startup(&missing, None).unwrap_err().to_string();
    assert!(err.contains("repo path does not exist"));

    let dir = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);
    let err = validate_startup(&cli, None).unwrap_err().to_string();
    assert!(err.contains("OPENAI_API_KEY"));
}

#[test]
fn startup_accepts_github_repo_link_and_still_requires_openai_key() {
    let cli = Cli::parse_from(["brf", "https://github.com/acme/widget.git"]);

    let err = validate_startup(&cli, None).unwrap_err().to_string();
    assert!(err.contains("OPENAI_API_KEY"));
    validate_startup(&cli, Some("sk-test")).unwrap();
}

#[test]
fn github_token_can_load_from_dotenv_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".env"),
        "\nGITHUB_TOKEN=\"ghp-from-dotenv\"\nOPENAI_API_KEY=sk-test\n",
    )
    .unwrap();

    let token = resolve_github_token(None, None, &[dir.path().to_path_buf()]).unwrap();

    assert_eq!(token.as_deref(), Some("ghp-from-dotenv"));
}

#[test]
fn github_pat_alias_can_load_from_dotenv_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".env"), "GITHUB_PAT=ghp-from-pat\n").unwrap();

    let token = resolve_github_token(None, None, &[dir.path().to_path_buf()]).unwrap();

    assert_eq!(token.as_deref(), Some("ghp-from-pat"));
}

#[test]
fn github_auth_header_uses_basic_auth_for_pat_tokens() {
    let header = github_authorization_header("ghp-test");

    assert_eq!(
        header,
        "AUTHORIZATION: Basic eC1hY2Nlc3MtdG9rZW46Z2hwLXRlc3Q="
    );
    assert!(!header.contains("ghp-test"));
}

#[test]
fn github_clone_error_mentions_dotenv_when_no_token_was_loaded() {
    let message = github_clone_error_message(
        "github.com/acme/widget",
        "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
        false,
    );

    assert!(message.contains("No GitHub token was loaded"));
    assert!(message.contains("GITHUB_TOKEN"));
    assert!(message.contains("GH_TOKEN"));
    assert!(message.contains("GITHUB_PAT"));
}

#[test]
fn github_repo_link_parses_owner_repo_and_clone_url() {
    let repo = GitHubRepo::parse("https://github.com/acme/widget.git").unwrap();

    assert_eq!(repo.owner, "acme");
    assert_eq!(repo.name, "widget");
    assert_eq!(repo.clone_url, "https://github.com/acme/widget.git");
    assert_eq!(repo.label, "github.com/acme/widget");
}

#[test]
fn github_repo_link_prepares_temp_clone_and_uses_token() {
    let dotenv_dir = tempfile::tempdir().unwrap();
    fs::write(dotenv_dir.path().join(".env"), "GITHUB_TOKEN=ghp-test\n").unwrap();
    let clone_root = tempfile::tempdir().unwrap();
    let token = resolve_github_token(None, None, &[dotenv_dir.path().to_path_buf()]).unwrap();
    let mut seen = None;

    let prepared = prepare_repo_source_with_cloner(
        std::path::Path::new("https://github.com/acme/widget"),
        clone_root.path(),
        token.as_deref(),
        |repo, destination, token| {
            seen = Some((
                repo.clone_url.clone(),
                destination.to_path_buf(),
                token.map(str::to_string),
            ));
            fs::create_dir_all(destination).unwrap();
            Ok(())
        },
    )
    .unwrap();

    let (clone_url, destination, token) = seen.unwrap();
    assert_eq!(clone_url, "https://github.com/acme/widget.git");
    assert_eq!(token.as_deref(), Some("ghp-test"));
    assert_eq!(prepared.local_path, destination);
    assert!(prepared.local_path.starts_with(clone_root.path()));
    assert_eq!(prepared.source_label, "github.com/acme/widget");
    assert!(prepared.temporary_clone);
}

#[test]
fn startup_can_load_openai_api_key_from_dotenv_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".env"),
        "\n# Branch to the Future credentials\nOPENAI_API_KEY=\"sk-from-dotenv\"\nOTHER=value\n",
    )
    .unwrap();
    let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);

    let api_key = resolve_openai_api_key(None, &[dir.path().to_path_buf()]).unwrap();

    assert_eq!(api_key, "sk-from-dotenv");
    validate_startup(&cli, Some(&api_key)).unwrap();
}

#[test]
fn environment_openai_api_key_wins_over_dotenv_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".env"), "OPENAI_API_KEY=sk-from-dotenv\n").unwrap();

    let api_key = resolve_openai_api_key(Some("sk-from-env"), &[dir.path().to_path_buf()]).unwrap();

    assert_eq!(api_key, "sk-from-env");
}

#[test]
fn scanner_detects_core_file_kinds_and_respects_ignore() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"dependencies":{"next":"15.0.0","react":"19.0.0"},"devDependencies":{"typescript":"5.0.0"}}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("app/api/upload")).unwrap();
    fs::write(
        dir.path().join("app/api/upload/route.ts"),
        "import { upload } from '@/lib/s3';\nexport async function POST() { return upload(); }\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("components")).unwrap();
    fs::write(
        dir.path().join("components/UploadForm.tsx"),
        "export function UploadForm() { const handleSubmit = () => {}; return null; }\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("workers")).unwrap();
    fs::write(
        dir.path().join("workers/parser.ts"),
        "export class Parser {}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("db")).unwrap();
    fs::write(
        dir.path().join("db/schema.sql"),
        "create table uploads(id text);\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();
    fs::write(
        dir.path().join("tests/upload.test.ts"),
        "test('upload', () => {});\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("tests/cli_tests.rs"),
        "#[test]\nfn cli_works() {}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("ignored")).unwrap();
    fs::write(
        dir.path().join("ignored/hidden.ts"),
        "export function Hidden() {}\n",
    )
    .unwrap();

    let model = scan_repo(dir.path(), 200_000, &["ignored".to_string()]).unwrap();

    assert!(model.frameworks.contains(&"Next.js".to_string()));
    assert!(model.frameworks.contains(&"TypeScript".to_string()));
    assert!(model
        .routes
        .iter()
        .any(|route| route.route == "/api/upload"));
    assert!(model
        .tests
        .iter()
        .any(|path| path == "tests/upload.test.ts"));
    assert!(model.tests.iter().any(|path| path == "tests/cli_tests.rs"));
    assert!(model.config_files.iter().any(|path| path == "package.json"));
    assert!(model
        .files
        .iter()
        .any(|file| file.path == "components/UploadForm.tsx"
            && file.symbols.contains(&"UploadForm".to_string())));
    assert!(model
        .files
        .iter()
        .any(|file| file.path == "app/api/upload/route.ts"
            && file.imports.contains(&"@/lib/s3".to_string())));
    assert!(!model
        .files
        .iter()
        .any(|file| file.path == "ignored/hidden.ts"));
}

#[test]
fn scanner_understands_bundled_resume_interview_sample() {
    let model = scan_repo(
        std::path::Path::new("sample-repos/resume-interview"),
        200_000,
        &[],
    )
    .unwrap();

    assert_eq!(model.repo_name, "resume-interview");
    assert!(model.frameworks.contains(&"Next.js".to_string()));
    assert!(model.frameworks.contains(&"React".to_string()));
    assert!(model
        .routes
        .iter()
        .any(|route| route.route == "/api/upload"));
    assert!(model
        .routes
        .iter()
        .any(|route| route.route == "/api/feedback"));
    assert!(model.tests.contains(&"tests/upload.test.ts".to_string()));
    assert!(model
        .files
        .iter()
        .any(|file| file.path == "workers/parser.ts"));
    assert!(model.files.iter().any(|file| file.path == "db/schema.sql"));
    assert!(model
        .risk_signals
        .iter()
        .any(|signal| signal.path == "app/api/upload/route.ts"));
}

#[test]
fn impact_parser_clamps_scores_and_rejects_unknown_enums() {
    let valid = r#"{
      "summary":"Async upload",
      "current_architecture":[{"label":"UPLOAD API","responsibilities":["accept resume uploads"],"files":["app/api/upload/route.ts"]}],
      "impact_path":[{"path":"app/api/upload/route.ts","reason":"entrypoint","impact_score":130,"confidence":101,"risk":"high","change_needed":"enqueue"}],
      "risk_summary":["PII"],
      "tests_to_add":["returns job id"],
      "futures":[{"name":"Proper Architecture","description":"queue worker","complexity":"high","risk":"low","architecture":[{"label":"QUEUE WORKER","responsibilities":["parse resumes async"],"files":["app/api/upload/route.ts"]}],"affected_files":["app/api/upload/route.ts"],"benefits":["robust"],"drawbacks":["more code"],"patch_plan":["add queue"],"test_plan":["status tests"]}],
      "recommended_future":"Proper Architecture"
    }"#;

    let parsed = parse_impact_analysis(valid).unwrap();
    assert_eq!(parsed.impact_path[0].impact_score, 100);
    assert_eq!(parsed.impact_path[0].confidence, 100);

    let invalid = valid.replace(r#""risk":"high""#, r#""risk":"severe""#);
    let err = parse_impact_analysis(&invalid).unwrap_err().to_string();
    assert!(err.contains("invalid impact analysis JSON"));
}

#[test]
fn impact_parser_normalizes_llm_shape_variants() {
    let gptish = r#"{
      "summary":"Async upload",
      "currentArchitecture":[{"name":"CLI ENTRY","responsibilities":"parse args","files":"src/main.rs"}],
      "impactPath":[{"file":"src/main.rs","reason":"entrypoint","impactScore":0.82,"confidence":"0.91","risk":"High","changeNeeded":"wire repo clone"}],
      "riskSummary":"token handling",
      "testsToAdd":"clone auth test",
      "futures":[{"name":"Minimal Patch","description":"queue worker","complexity":"Medium","risk":"Low","architecture":[{"stage":"AUTH CLONE","responsibilities":"load token","files":["src/repo_source.rs"]}],"affectedFiles":"src/repo_source.rs","benefits":"works for private repos","drawbacks":"more auth paths","patchPlan":"add token header","testPlan":"clone private repo"}],
      "recommendedFuture":"Minimal Patch"
    }"#;

    let parsed = parse_impact_analysis(gptish).unwrap();

    assert_eq!(parsed.current_architecture[0].label, "CLI ENTRY");
    assert_eq!(parsed.current_architecture[0].files, ["src/main.rs"]);
    assert_eq!(parsed.impact_path[0].path, "src/main.rs");
    assert_eq!(parsed.impact_path[0].impact_score, 82);
    assert_eq!(parsed.impact_path[0].confidence, 91);
    assert_eq!(parsed.impact_path[0].risk, RiskLevel::High);
    assert_eq!(parsed.risk_summary, ["token handling"]);
    assert_eq!(parsed.tests_to_add, ["clone auth test"]);
    assert_eq!(parsed.futures[0].complexity, Complexity::Medium);
    assert_eq!(parsed.futures[0].risk, RiskLevel::Low);
    assert_eq!(parsed.futures[0].affected_files, ["src/repo_source.rs"]);
    assert_eq!(parsed.futures[0].patch_plan, ["add token header"]);
    assert_eq!(parsed.futures[0].architecture[0].label, "AUTH CLONE");
}

#[test]
fn markdown_export_contains_required_sections() {
    let dir = tempfile::tempdir().unwrap();
    let analysis = sample_analysis();

    let path = export_markdown(
        dir.path(),
        "session-1",
        "add async resume parsing",
        "sample",
        &analysis,
        0,
    )
    .unwrap();
    assert_eq!(path.file_name().unwrap(), "branch-to-the-future-report.md");
    let report = fs::read_to_string(path).unwrap();

    for heading in [
        "# Branch to the Future Report",
        "## Change Request",
        "## Repo Summary",
        "## Impact Path",
        "## Affected Files",
        "## Risk Summary",
        "## Branch to the Future",
        "## Recommended Path",
        "## Test Plan",
        "## Patch Skeleton",
        "## Architecture Scaffold",
    ] {
        assert!(report.contains(heading), "missing {heading}");
    }
    assert!(report.contains("terminal-native layout"));
}

#[test]
fn default_output_dir_is_target_repo_root() {
    let dir = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);

    assert_eq!(cli.resolved_output_dir(), dir.path());
}

#[tokio::test]
async fn ai_client_sends_responses_request_and_retries_malformed_json() {
    let (url, bodies_rx) = fake_openai_server();
    let client = OpenAiClient::with_base_url(
        "test-key".to_string(),
        url,
        "gpt-5.5".to_string(),
        ReasoningEffort::High,
    );
    let repo_model = RepoModel {
        repo_name: "sample".to_string(),
        root_path: "/tmp/sample".to_string(),
        frameworks: vec!["Next.js".to_string()],
        files: vec![],
        routes: vec![],
        tests: vec![],
        config_files: vec![],
        risk_signals: vec![],
    };

    let analysis = client
        .analyze(&repo_model, "add async resume parsing")
        .await
        .unwrap();

    assert_eq!(analysis.recommended_future, "Minimal Patch");
    let first_body: serde_json::Value = serde_json::from_str(&bodies_rx.recv().unwrap()).unwrap();
    assert_eq!(first_body["model"], "gpt-5.5");
    assert_eq!(first_body["reasoning"]["effort"], "high");
    assert_eq!(first_body["max_output_tokens"], 8_000);
    assert_eq!(first_body["text"]["format"]["type"], "json_schema");
    assert!(first_body["text"]["format"]["schema"]["required"]
        .to_string()
        .contains("current_architecture"));
    assert!(
        first_body["text"]["format"]["schema"]["properties"]["futures"]["items"]["required"]
            .to_string()
            .contains("architecture")
    );
    assert!(first_body["input"]
        .to_string()
        .contains("add async resume parsing"));
    let second_body: serde_json::Value = serde_json::from_str(&bodies_rx.recv().unwrap()).unwrap();
    assert_eq!(second_body["max_output_tokens"], 16_000);
    assert!(second_body
        .to_string()
        .contains("Previous response failed JSON parsing"));
}

#[tokio::test]
async fn ai_client_streams_openai_text_deltas_and_parses_final_json() {
    let (url, bodies_rx) = fake_openai_stream_server();
    let client = OpenAiClient::with_base_url(
        "test-key".to_string(),
        url,
        "gpt-5.5".to_string(),
        ReasoningEffort::High,
    );
    let repo_model = RepoModel {
        repo_name: "sample".to_string(),
        root_path: "/tmp/sample".to_string(),
        frameworks: vec!["Next.js".to_string()],
        files: vec![],
        routes: vec![],
        tests: vec![],
        config_files: vec![],
        risk_signals: vec![],
    };
    let mut deltas = Vec::new();

    let analysis = client
        .analyze_streaming(&repo_model, "add async resume parsing", |event| {
            if let AiStreamEvent::TextDelta(delta) = event {
                deltas.push(delta.to_string());
            }
        })
        .await
        .unwrap();

    assert_eq!(analysis.recommended_future, "Minimal Patch");
    assert!(deltas.iter().any(|delta| delta.contains("\"summary\"")));
    let body: serde_json::Value = serde_json::from_str(&bodies_rx.recv().unwrap()).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["text"]["format"]["type"], "json_schema");
}

#[tokio::test]
async fn ai_client_stream_retry_expands_output_tokens_after_truncated_json() {
    let (url, bodies_rx) = fake_openai_truncated_stream_then_valid_server();
    let client = OpenAiClient::with_base_url(
        "test-key".to_string(),
        url,
        "gpt-5.5".to_string(),
        ReasoningEffort::High,
    )
    .with_limits(2_500, 80);
    let repo_model = RepoModel {
        repo_name: "locator".to_string(),
        root_path: "/tmp/locator".to_string(),
        frameworks: vec!["Rust".to_string()],
        files: vec![],
        routes: vec![],
        tests: vec![],
        config_files: vec![],
        risk_signals: vec![],
    };

    let analysis = client
        .analyze_streaming(&repo_model, "add github analysis", |_| {})
        .await
        .unwrap();

    assert_eq!(analysis.recommended_future, "Minimal Patch");
    let first_body: serde_json::Value = serde_json::from_str(&bodies_rx.recv().unwrap()).unwrap();
    assert_eq!(first_body["max_output_tokens"], 2_500);
    let second_body: serde_json::Value = serde_json::from_str(&bodies_rx.recv().unwrap()).unwrap();
    assert_eq!(second_body["max_output_tokens"], 8_000);
    assert!(second_body
        .to_string()
        .contains("Previous response failed JSON parsing"));
}

fn fake_openai_server() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let body = read_http_body(&mut stream);
            tx.send(body).unwrap();
            let output = if index == 0 {
                "not json".to_string()
            } else {
                valid_analysis_json()
            };
            let response_body = serde_json::json!({
                "output": [{
                    "content": [{
                        "type": "output_text",
                        "text": output
                    }]
                }]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{address}"), rx)
}

fn fake_openai_stream_server() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let body = read_http_body(&mut stream);
        tx.send(body).unwrap();
        let json = valid_analysis_json();
        let split = json.len() / 2;
        let first = &json[..split];
        let second = &json[split..];
        let response_body = format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\n",
            serde_json::json!({"type":"response.output_text.delta","delta":first}),
            serde_json::json!({"type":"response.output_text.delta","delta":second}),
            serde_json::json!({"type":"response.completed"})
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (format!("http://{address}"), rx)
}

fn fake_openai_truncated_stream_then_valid_server() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let body = read_http_body(&mut stream);
            tx.send(body).unwrap();
            let output = if index == 0 {
                r#"{"summary":"partial""#.to_string()
            } else {
                valid_analysis_json()
            };
            let response_body = format!(
                "data: {}\n\ndata: {}\n\n",
                serde_json::json!({"type":"response.output_text.delta","delta":output}),
                serde_json::json!({"type":"response.completed"})
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{address}"), rx)
}

fn read_http_body(stream: &mut std::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut temp = [0; 1024];
    loop {
        let count = stream.read(&mut temp).unwrap();
        buffer.extend_from_slice(&temp[..count]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let header_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length: ")
                .map(|_| line.split_once(':').unwrap().1.trim())
        })
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let count = stream.read(&mut temp).unwrap();
        body.extend_from_slice(&temp[..count]);
    }
    String::from_utf8(body[..content_length].to_vec()).unwrap()
}

fn valid_analysis_json() -> String {
    r#"{
      "summary":"Async upload",
      "current_architecture":[{"label":"UPLOAD API","responsibilities":["accept resume uploads"],"files":["app/api/upload/route.ts"]}],
      "impact_path":[],
      "risk_summary":["PII"],
      "tests_to_add":["returns job id"],
      "futures":[{"name":"Minimal Patch","description":"queue worker","complexity":"medium","risk":"medium","architecture":[{"label":"ASYNC WORKER","responsibilities":["process uploads"],"files":["workers/parser.ts"]}],"affected_files":[],"benefits":["small"],"drawbacks":["basic"],"patch_plan":["add status"],"test_plan":["status test"]}],
      "recommended_future":"Minimal Patch"
    }"#
    .to_string()
}

fn sample_analysis() -> ImpactAnalysis {
    ImpactAnalysis {
        summary: "Async upload".to_string(),
        current_architecture: vec![],
        impact_path: vec![],
        risk_summary: vec!["PII storage".to_string()],
        tests_to_add: vec!["upload returns job_id".to_string()],
        futures: vec![ImplementationFuture {
            name: "Minimal Patch".to_string(),
            description: "Add worker handoff".to_string(),
            complexity: Complexity::Medium,
            risk: RiskLevel::Medium,
            architecture: vec![],
            affected_files: vec!["app/api/upload/route.ts".to_string()],
            benefits: vec!["small diff".to_string()],
            drawbacks: vec!["polling remains basic".to_string()],
            patch_plan: vec!["return job id".to_string()],
            test_plan: vec!["status endpoint".to_string()],
        }],
        recommended_future: "Minimal Patch".to_string(),
    }
}
