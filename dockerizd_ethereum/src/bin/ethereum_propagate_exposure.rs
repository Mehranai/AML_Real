use clap::Parser;
use ethereum_aml::{
    config::AppConfig,
    db::initialize_ethereum_schema,
    exposure::{ExposureOptions, propagate_exposure},
    init_tracing,
};

#[derive(Debug, Parser)]
#[command(
    name = "ethereum_propagate_exposure",
    about = "Propagate reviewed illicit-seed exposure over canonical Ethereum value-flow paths"
)]
struct Cli {
    #[arg(long, default_value_t = 5)]
    max_hops: u8,
    #[arg(long, default_value_t = 0.65)]
    hop_decay: f64,
    #[arg(long, default_value_t = 365.0)]
    time_half_life_days: f64,
    #[arg(long, default_value_t = 3)]
    max_paths_per_subject: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;
    let report = propagate_exposure(
        &config,
        ExposureOptions {
            max_hops: cli.max_hops,
            hop_decay: cli.hop_decay,
            time_half_life_days: cli.time_half_life_days,
            max_paths_per_subject: cli.max_paths_per_subject,
        },
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
