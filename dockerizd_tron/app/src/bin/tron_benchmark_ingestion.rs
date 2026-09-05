use std::env;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use arz_axum_for_services::{
    config::AppConfig,
    db::tron_schema::validate_tron_schema,
    services::{loader::LoaderTron, tron::performance::run_historical_benchmark},
    utils::tron_address::normalize_tron_address,
};
use clickhouse::Client;

const MAX_BENCHMARK_BLOCKS: u64 = 10_000;

#[tokio::main]
async fn main() -> Result<()> {
    let (start_block, end_block, investigation_address) = parse_args(env::args().skip(1))?;
    let config = AppConfig::from_env();
    let admin_client = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass);

    validate_tron_schema(&admin_client).await?;

    let loader = Arc::new(LoaderTron::new(&config).await?);
    let report = run_historical_benchmark(
        &config,
        loader,
        start_block,
        end_block,
        investigation_address,
    )
    .await?;

    println!("{}", serde_json::to_string_pretty(&report)?);

    if report.status == "FAILED" {
        return Err(anyhow!(
            "TRON historical benchmark failed: {}",
            report.error_message
        ));
    }

    Ok(())
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(u64, u64, Option<String>)> {
    let start = args.next().ok_or_else(usage)?;
    let end = args.next().ok_or_else(usage)?;
    let address = args.next();

    if args.next().is_some() {
        return Err(usage());
    }

    let start_block = start
        .parse::<u64>()
        .with_context(|| format!("invalid start block: {start}"))?;
    let end_block = end
        .parse::<u64>()
        .with_context(|| format!("invalid end block: {end}"))?;
    let block_count = end_block
        .checked_sub(start_block)
        .and_then(|difference| difference.checked_add(1))
        .ok_or_else(|| anyhow!("benchmark end block must be at or above the start block"))?;

    if block_count > MAX_BENCHMARK_BLOCKS {
        return Err(anyhow!(
            "benchmark range contains {block_count} blocks; maximum is {MAX_BENCHMARK_BLOCKS}"
        ));
    }

    let address = address
        .map(|value| {
            normalize_tron_address(&value)
                .ok_or_else(|| anyhow!("invalid TRON investigation address: {value}"))
        })
        .transpose()?;

    Ok((start_block, end_block, address))
}

fn usage() -> anyhow::Error {
    anyhow!(
        "usage: cargo run --bin tron_benchmark_ingestion -- <start_block> <end_block> [wallet_address]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_range() {
        assert_eq!(
            parse_args(["100".to_string(), "109".to_string()].into_iter()).unwrap(),
            (100, 109, None)
        );
    }

    #[test]
    fn rejects_reversed_and_oversized_ranges() {
        assert!(parse_args(["2".to_string(), "1".to_string()].into_iter()).is_err());
        assert!(parse_args(["1".to_string(), "10001".to_string()].into_iter()).is_err());
    }
}
