use std::path::PathBuf;

use clap::Parser;
use ethereum_aml::{
    config::AppConfig,
    db::initialize_ethereum_schema,
    init_tracing,
    intelligence::{
        EntityLabelImportOptions, IntelligenceSourceRegistration, import_entity_labels_csv,
    },
};

#[derive(Debug, Parser)]
#[command(
    name = "ethereum_ingest_entity_labels",
    about = "Import reviewed Ethereum entity labels from a structured CSV file"
)]
struct Cli {
    #[arg(long)]
    file: PathBuf,
    #[arg(long)]
    source_id: String,
    #[arg(long)]
    source_name: String,
    #[arg(long, default_value = "ANALYST")]
    source_type: String,
    #[arg(long, default_value = "UNVERIFIED")]
    trust_tier: String,
    #[arg(long, default_value = "")]
    reference_url: String,
    #[arg(long)]
    created_by: String,
    #[arg(long)]
    submitted_by: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;
    let report = import_entity_labels_csv(
        &config,
        &cli.file,
        EntityLabelImportOptions {
            source: IntelligenceSourceRegistration {
                source_id: cli.source_id,
                source_name: cli.source_name,
                source_type: cli.source_type,
                trust_tier: cli.trust_tier,
                reference_url: cli.reference_url,
                created_by: cli.created_by,
            },
            submitted_by: cli.submitted_by,
        },
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
