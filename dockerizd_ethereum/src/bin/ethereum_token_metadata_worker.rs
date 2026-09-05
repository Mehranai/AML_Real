use clap::Parser;
use ethereum_aml::{
    config::AppConfig, db::initialize_ethereum_schema, ethereum::run_token_metadata_worker,
    init_tracing,
};

#[derive(Debug, Parser)]
#[command(
    name = "ethereum_token_metadata_worker",
    about = "Resolve discovered Ethereum token metadata from on-chain contracts"
)]
struct Cli {
    /// Process the current bounded batch and exit.
    #[arg(long)]
    once: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;
    run_token_metadata_worker(config, cli.once).await
}
