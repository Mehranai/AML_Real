use std::env;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use arz_axum_for_services::{
    config::AppConfig,
    db::tron_schema::validate_tron_schema,
    services::{loader::LoaderTron, tron::fetcher::replay_tron_range},
};
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let (start_block, end_block) = parse_range(env::args().skip(1))?;
    let config = AppConfig::from_env();
    let admin_client = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass);

    validate_tron_schema(&admin_client).await?;

    let loader = Arc::new(LoaderTron::new(&config).await?);

    println!(
        "[TRON REPLAY] replaying finalized block range {}..={}",
        start_block, end_block
    );
    replay_tron_range(loader, start_block, end_block).await?;
    println!("[TRON REPLAY] completed successfully.");

    Ok(())
}

fn parse_range(mut args: impl Iterator<Item = String>) -> Result<(u64, u64)> {
    let start = args.next().ok_or_else(usage)?;
    let end = args.next();

    if args.next().is_some() {
        return Err(usage());
    }

    let start_block = start
        .parse::<u64>()
        .with_context(|| format!("invalid start block: {start}"))?;
    let end_block = end
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("invalid end block: {value}"))
        })
        .transpose()?
        .unwrap_or(start_block);

    Ok((start_block, end_block))
}

fn usage() -> anyhow::Error {
    anyhow!("usage: cargo run --bin tron_replay_blocks -- <start_block> [end_block]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_block_and_inclusive_range() {
        assert_eq!(
            parse_range(["100".to_string()].into_iter()).unwrap(),
            (100, 100)
        );
        assert_eq!(
            parse_range(["100".to_string(), "105".to_string()].into_iter()).unwrap(),
            (100, 105)
        );
    }

    #[test]
    fn rejects_missing_or_extra_arguments() {
        assert!(parse_range(Vec::<String>::new().into_iter()).is_err());
        assert!(
            parse_range(["1".to_string(), "2".to_string(), "3".to_string()].into_iter()).is_err()
        );
    }
}
