use anyhow::{Context, Result, anyhow};
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::services::tron::address_clustering::discover_address_cluster_claims;
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() > 3 {
        return Err(anyhow!(
            "usage: tron_discover_address_clusters [start_block] [end_block] [max_claims]"
        ));
    }
    let start_block = parse_u64(args.first(), "start_block")?.unwrap_or(0);
    let max_claims = parse_u64(args.get(2), "max_claims")?.unwrap_or(5_000) as usize;
    let config = AppConfig::from_env();
    let clickhouse = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass)
        .with_database(&config.clickhouse_db_tron);
    let end_block = match parse_u64(args.get(1), "end_block")? {
        Some(value) => value,
        None => clickhouse
            .query("SELECT ifNull(max(block_number), 0) FROM ingested_blocks FINAL WHERE chain = 'tron' AND ingestion_status = 'COMPLETE'")
            .fetch_one::<u64>()
            .await
            .context("failed to determine the latest completed TRON block")?,
    };
    if start_block > end_block {
        return Err(anyhow!(
            "start_block must be less than or equal to end_block"
        ));
    }

    let report =
        discover_address_cluster_claims(&clickhouse, start_block, end_block, max_claims).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn parse_u64(value: Option<&String>, field: &str) -> Result<Option<u64>> {
    value
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("{field} must be an unsigned integer"))
        })
        .transpose()
}
