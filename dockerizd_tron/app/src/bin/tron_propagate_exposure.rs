use std::sync::Arc;

use anyhow::Result;
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::tasks::exposure_task::run_all_exposure_scans;
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::from_env();
    let max_hops = std::env::var("TRON_EXPOSURE_MAX_HOPS")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(5)
        .clamp(1, 10);
    let clickhouse = Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron),
    );
    let (seed_count, exposure_count) = run_all_exposure_scans(clickhouse, max_hops).await?;

    println!(
        "[TRON EXPOSURE] completed seeds={} exposure_rows={} max_hops={}",
        seed_count, exposure_count, max_hops
    );

    Ok(())
}
