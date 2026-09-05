use std::fs;

use anyhow::{Context, Result, anyhow};
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::services::tron::entity_intelligence::{
    IntelligenceSourceInput, register_intelligence_source,
};
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: tron_register_intelligence_source <source.json>"))?;
    let input: IntelligenceSourceInput = serde_json::from_str(
        &fs::read_to_string(&path)
            .with_context(|| format!("failed to read intelligence source file {path}"))?,
    )
    .with_context(|| format!("invalid intelligence source JSON in {path}"))?;
    let config = AppConfig::from_env();
    let clickhouse = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass)
        .with_database(&config.clickhouse_db_tron);
    let row = register_intelligence_source(&clickhouse, input).await?;

    println!(
        "[TRON INTELLIGENCE SOURCE] source_id={} trust_tier={} active={}",
        row.source_id, row.trust_tier, row.is_active
    );
    Ok(())
}
