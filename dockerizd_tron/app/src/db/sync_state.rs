use clickhouse::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize, clickhouse::Row)]
struct SyncStateRow {
    last_synced_block: u64,
}

pub async fn get_last_synced_block(client: &Client) -> anyhow::Result<Option<u64>> {
    let row = client
        .query(
            "SELECT argMax(last_synced_block, updated_at) AS last_synced_block
             FROM sync_state
             WHERE chain = 'tron'
             HAVING count() > 0",
        )
        .fetch_optional::<SyncStateRow>()
        .await?;

    Ok(row.map(|state| state.last_synced_block))
}
