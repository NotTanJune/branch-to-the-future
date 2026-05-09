use anyhow::Result;
use branch_futures::{
    app::App,
    cli::{load_openai_api_key, validate_startup, Cli},
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let api_key = load_openai_api_key(&cli)?;
    validate_startup(&cli, Some(&api_key))?;
    App::from_cli(cli, api_key).run().await
}
