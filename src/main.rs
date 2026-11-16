use anyhow::Result;
use branch_futures::{
    app::App,
    blast::run_blast_json,
    cli::{dotenv_search_dirs, load_openai_api_key, validate_startup, Cli, CliCommand},
    machine::{run_analyze_json, run_scan_json, write_json},
    repo_source::prepare_repo_source,
    watch::run_watch,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.clone() {
        Some(CliCommand::Scan(scan)) => {
            let envelope = run_scan_json(&scan)?;
            write_json(&envelope)?;
            return Ok(());
        }
        Some(CliCommand::Analyze(analyze)) => {
            let envelope = run_analyze_json(&analyze).await?;
            write_json(&envelope)?;
            return Ok(());
        }
        Some(CliCommand::Blast(blast)) => {
            let report = run_blast_json(&blast)?;
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            serde_json::to_writer_pretty(&mut handle, &report)?;
            std::io::Write::write_all(&mut handle, b"\n")?;
            return Ok(());
        }
        Some(CliCommand::Watch(watch)) => {
            return run_watch(watch).await;
        }
        None => {}
    }
    let api_key = load_openai_api_key(&cli)?;
    validate_startup(&cli, Some(&api_key))?;
    let search_dirs = dotenv_search_dirs(&cli)?;
    let prepared_repo = prepare_repo_source(cli.tui_repo_path()?, &search_dirs)?;
    App::from_prepared_cli(cli, api_key, prepared_repo)
        .run()
        .await
}
