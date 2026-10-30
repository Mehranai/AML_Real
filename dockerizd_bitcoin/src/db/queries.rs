//! Canonical state, publication and coverage queries.
use super::*;
use anyhow::ensure;
use serde_json::{Value, json};
impl Database {
    pub async fn state(&self) -> Result<SyncState> {
        Ok(crate::warehouse::from_state(
            aml_warehouse::publication::state(
                &self.warehouse_connection(),
                self.database(),
                aml_warehouse::Network::Bitcoin,
            )
            .await?,
        ))
    }
    pub async fn legacy_state(&self) -> Result<SyncState> {
        let tables: Vec<Value> = self.rows("SELECT count() AS n FROM system.tables WHERE database=currentDatabase() AND name IN ('bitcoin_sync_state','bitcoin_sync_current','bitcoin_block_commits','bitcoin_blocks_canonical','bitcoin_transactions','bitcoin_transactions_canonical','bitcoin_inputs','bitcoin_inputs_canonical','bitcoin_outputs','bitcoin_outputs_canonical','bitcoin_utxos')", &[]).await?;
        if tables[0]["n"] == 0 {
            return Ok(SyncState::default());
        }
        ensure!(
            tables[0]["n"] == 11,
            "incomplete Bitcoin legacy schema; restore its missing objects before backfill"
        );
        Ok(self
            .rows("SELECT * FROM bitcoin_sync_current", &[])
            .await?
            .pop()
            .unwrap_or_default())
    }
    pub async fn publish(&self, state: &SyncState) -> Result<()> {
        self.publish_after(&self.state().await?, state).await
    }
    pub async fn publish_after(&self, previous: &SyncState, state: &SyncState) -> Result<()> {
        aml_warehouse::publication::publish(
            &self.warehouse_connection(),
            self.database(),
            aml_warehouse::Network::Bitcoin,
            &crate::warehouse::to_state(previous),
            &crate::warehouse::to_state(state),
        )
        .await
    }
    pub async fn next_revision(&self, previous: u64) -> Result<u64> {
        // Advance past both the publication fence and unpublished reservations.
        aml_warehouse::publication::next_revision(
            &self.warehouse_connection(),
            self.database(),
            aml_warehouse::Network::Bitcoin,
            previous.max(timestamp().saturating_mul(1000)),
        )
        .await
    }
    pub async fn block_hash(&self, height: u64) -> Result<Option<String>> {
        let rows = aml_warehouse::core::read(
            &self.warehouse_connection(),
            self.database(),
            aml_warehouse::Network::Bitcoin,
            aml_warehouse::core::View::Blocks,
            &aml_warehouse::core::Query {
                from: Some(height),
                to: Some(height),
                limit: 1,
            },
        )
        .await?;
        Ok(rows
            .first()
            .and_then(|v| v["block_hash"].as_str())
            .map(str::to_string))
    }
    pub async fn status(&self) -> Result<Value> {
        let s = self.state().await?;
        let snapshot =
            aml_warehouse::visibility::Snapshot::from_state(crate::warehouse::to_state(&s))?;
        let failures = crate::failures::summary(self, &s).await?;
        let metadata = crate::assets::read(self).await?;
        aml_warehouse::visibility::unchanged(
            &self.warehouse_connection(),
            self.database(),
            &snapshot,
        )
        .await?;
        Ok(
            json!({"api_version":"1.0","network_id":crate::NETWORK_ID,"graph_storage":"main_vm",
            "initialized":s.initialized == 1,"last_synced_block":if s.initialized == 1 {Some(s.tip_height)} else {None},
            "block_hash":s.tip_hash,"start_block":s.start_height,"history_from_genesis":s.initialized == 1 && s.start_height == 0,
            "observed_rpc_height":s.observed_rpc_height,"revision":s.revision.to_string(),"published_at_unix_ms":s.published_at,
            "coverage_scope":"confirmed_ingested_blocks","mempool_included":false,
            "source_snapshot":snapshot.value(),"ingestion_failures":failures,"native_asset_snapshot":metadata["native_asset_snapshot"],"analytics_worker":crate::analytics::local_status(self.database())}),
        )
    }
}
