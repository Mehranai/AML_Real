use std::sync::Arc;

use crate::config::SyncMode;
use crate::helper::tron::TronClient;

pub async fn resolve_start_block_tron(
    sync_mode: &SyncMode,
    tron_client: Arc<TronClient>,
    config_start_block: u64,
    last_synced: Option<u64>,
) -> anyhow::Result<u64> {
    match sync_mode {
        SyncMode::Backfill => Ok(config_start_block),
        SyncMode::Live => {
            let latest = tron_client.get_block_number().await?;
            Ok(latest.saturating_sub(20))
        }
        SyncMode::Auto => Ok(last_synced
            .map(|block| block.saturating_add(1))
            .unwrap_or(config_start_block)),
    }
}
