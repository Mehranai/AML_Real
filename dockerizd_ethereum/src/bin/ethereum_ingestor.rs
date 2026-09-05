use clap::{Parser, Subcommand};
use ethereum_aml::{
    config::AppConfig,
    db::initialize_ethereum_schema,
    ethereum::{IngestionService, probe_node},
    init_tracing,
};

#[derive(Debug, Parser)]
#[command(
    name = "ethereum_ingestor",
    about = "Canonical finalized Ethereum ingestion for the AML warehouse"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Verify provider, chain identity, finality, and ingestion capabilities.
    Probe,
    /// Ingest an explicit inclusive finalized block range.
    Range {
        #[arg(long)]
        from_block: u64,
        #[arg(long)]
        to_block: u64,
    },
    /// Continue from the checkpoint and follow the finalized chain.
    Follow {
        /// Override the checkpoint/start block for this run.
        #[arg(long)]
        start_block: Option<u64>,
        /// Stop after this many blocks; omit to run continuously.
        #[arg(long)]
        max_blocks: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = AppConfig::from_env()?;

    match cli.command.unwrap_or(Command::Probe) {
        Command::Probe => {
            let status = probe_node(&config).await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Command::Range {
            from_block,
            to_block,
        } => {
            initialize_ethereum_schema(&config).await?;
            let service = IngestionService::connect(config).await?;
            let report = service.ingest_range(from_block, to_block).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Follow {
            start_block,
            max_blocks,
        } => {
            initialize_ethereum_schema(&config).await?;
            let service = IngestionService::connect(config).await?;
            let report = service.follow_finalized(start_block, max_blocks).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }

    Ok(())
}
