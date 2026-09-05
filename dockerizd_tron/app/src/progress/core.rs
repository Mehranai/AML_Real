use std::sync::Arc;

use anyhow::Result;
use clickhouse::Client;

use crate::models::token_metadata::TokenMetadataRow;

pub async fn save_token_metadata(clickhouse: Arc<Client>, row: TokenMetadataRow) -> Result<()> {
    let existing = clickhouse
        .query("SELECT count() FROM token_metadata WHERE token_address = ?")
        .bind(&row.token_address)
        .fetch_one::<u64>()
        .await?;

    if existing > 0 {
        return Ok(());
    }

    let mut insert = clickhouse
        .insert::<TokenMetadataRow>("token_metadata")
        .await?;
    insert.write(&row).await?;
    insert.end().await?;
    Ok(())
}

#[derive(Debug, clickhouse::Row, serde::Serialize)]
struct SyncStateRow {
    chain: String,
    last_synced_block: u64,
}

pub async fn save_sync_state(clickhouse: Arc<Client>, last_synced_block: u64) -> Result<()> {
    let mut insert = clickhouse.insert::<SyncStateRow>("sync_state").await?;
    insert
        .write(&SyncStateRow {
            chain: "tron".to_string(),
            last_synced_block,
        })
        .await?;
    insert.end().await?;
    Ok(())
}
