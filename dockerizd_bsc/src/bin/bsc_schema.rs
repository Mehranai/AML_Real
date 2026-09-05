use std::process::ExitCode;

use bsc_aml::{
    config::ClickHouseConfig,
    db::{initialize_bsc_schema, validate_bsc_schema},
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("BSC schema operation failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let check_only = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some("--help" | "-h") => {
            println!(
                "Usage: bsc_schema [--check]\nWithout --check, applies migrations then validates the schema."
            );
            return Ok(());
        }
        Some(_) => return Err("unknown argument; use --help".into()),
    };
    if std::env::args().nth(2).is_some() {
        return Err("too many arguments; use --help".into());
    }

    let config = ClickHouseConfig::from_env()?;
    tracing::info!(database = %config.database(), check_only, "validating BSC ClickHouse schema");
    let summary = if check_only {
        validate_bsc_schema(&config).await?
    } else {
        initialize_bsc_schema(&config).await?
    };
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
