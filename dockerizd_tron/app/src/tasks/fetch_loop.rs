use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clickhouse::Client;
use tokio::time::sleep;

use crate::config::{AppConfig, SyncMode};
use crate::db::sync_state::get_last_synced_block;
use crate::db::tron_schema::validate_tron_schema;
use crate::services::loader::LoaderTron;
use crate::services::sync_logic::resolve_start_block_tron;
use crate::services::tron;
use crate::services::tron::ingestion_state::FinalizedHashConflict;

pub async fn run_tron_loop(config: AppConfig) -> Result<()> {
    println!("[TRON] Starting ingestion");

    let admin_client = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass);

    // برای بررسی اینکه SQl با مدل Rust ناسازگار نباشد
    validate_tron_schema(&admin_client).await?;

    // برای اشتراک گذاری تسک بین async
    let loader = Arc::new(LoaderTron::new(&config).await?);

    // آخرین بلاک ذخیره شده از clickhouse گرفته میشه
    let last_synced = get_last_synced_block(&loader.clickhouse).await?;
    let start_block = resolve_start_block_tron(
        &config.sync_mode,
        loader.tron_client.clone(),
        config.tron_start_block,
        last_synced,
    )
    .await?;

    println!(
        "[TRON] sync_mode={:?} start_block={} last_synced={:?}",
        config.sync_mode, start_block, last_synced
    );

    if matches!(config.sync_mode, SyncMode::Backfill) {
        tron::fetcher::fetch_tron(loader, start_block, config.total_tron_txs).await?;
        println!("[TRON] Backfill pass finished successfully");
        return Ok(());
    }

    let mut next_block = start_block;
    let mut retry_delay_seconds = 1_u64;
    let poll_interval_seconds = config.tron_poll_interval_seconds.max(1);

    // لوپ اجرای اصلی برنامه
    loop {
        match tron::fetcher::fetch_tron(loader.clone(), next_block, config.total_tron_txs).await {
            Ok(()) => {
                retry_delay_seconds = 1;
                if let Some(last_synced) = get_last_synced_block(&loader.clickhouse).await? {
                    next_block = last_synced.saturating_add(1);
                }
                sleep(Duration::from_secs(poll_interval_seconds)).await;
            }
            Err(error) => {
                if error.downcast_ref::<FinalizedHashConflict>().is_some() {
                    eprintln!(
                        "[TRON] ingestion stopped because finalized block evidence conflicts: {error:#}"
                    );
                    return Err(error);
                }

                eprintln!(
                    "[TRON] ingestion failed at block {next_block}: {error:#}; retrying in {retry_delay_seconds} second(s)"
                );
                sleep(Duration::from_secs(retry_delay_seconds)).await;
                retry_delay_seconds = (retry_delay_seconds * 2).min(60);
            }
        }
    }
}
