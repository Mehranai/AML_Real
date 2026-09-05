use std::process::ExitCode;

use bsc_aml::{config::AppConfig, rpc::BscRpcProbe};
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
            eprintln!("BSC RPC probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut pretty = true;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--compact" => pretty = false,
            "--help" | "-h" => {
                println!(
                    "Usage: bsc_rpc_probe [--compact]\nReads BSC_* settings from environment, falling back to .env."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }

    let config = AppConfig::from_env()?;
    tracing::info!(mode = %config.mode, provider = %config.rpc_provider, "probing BSC RPC capabilities");
    let report = BscRpcProbe::new(config)?.inspect().await?;
    if pretty {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", serde_json::to_string(&report)?);
    }
    report.ensure_requirements()?;
    Ok(())
}
