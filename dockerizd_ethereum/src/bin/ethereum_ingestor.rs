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
    /// Bounded operator tools: probe, failures, scan-gaps, replay, repair.
    Maintenance {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
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
    if let Some(Command::Maintenance { args }) = &cli.command {
        if args.is_empty() || args == &["--help"] {
            println!("{}", aml_warehouse::maintenance::Command::help());
            return Ok(());
        }
        aml_warehouse::maintenance::Command::parse(args)?;
    }
    let config = AppConfig::from_env()?;

    match cli.command.unwrap_or(Command::Probe) {
        Command::Maintenance { args } => {
            if args.is_empty() || args == ["--help"] {
                println!("{}", aml_warehouse::maintenance::Command::help());
                return Ok(());
            }
            let command = aml_warehouse::maintenance::Command::parse(&args)?;
            let value = if command.action == "probe" {
                serde_json::to_value(probe_node(&config).await?)?
            } else if !command.mutates() {
                aml_warehouse::maintenance::read(
                    &ethereum_aml::db::database_client(&config),
                    &config.clickhouse_database,
                    aml_warehouse::Network::Ethereum,
                    &command,
                    config.eth_trace_mode.required(),
                )
                .await?
            } else {
                IngestionService::connect(config)
                    .await?
                    .maintenance(&command)
                    .await?
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
            anyhow::ensure!(
                command.action != "probe" || value["status"] == "ready",
                "RPC probe requires review; inspect the JSON report"
            );
        }
        Command::Probe => {
            let status = probe_node(&config).await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            anyhow::ensure!(
                status.status == "ready",
                "RPC probe requires review; inspect coverage"
            );
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
