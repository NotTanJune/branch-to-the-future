use anyhow::Result;
use branch_futures::{
    app::App,
    cli::{dotenv_search_dirs, load_openai_api_key, validate_startup, Cli},
    repo_source::prepare_repo_source,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let api_key = load_openai_api_key(&cli)?;
    validate_startup(&cli, Some(&api_key))?;
    let search_dirs = dotenv_search_dirs(&cli)?;
    let prepared_repo = prepare_repo_source(&cli.repo_path, &search_dirs)?;
    App::from_prepared_cli(cli, api_key, prepared_repo)
        .run()
        .await
}
