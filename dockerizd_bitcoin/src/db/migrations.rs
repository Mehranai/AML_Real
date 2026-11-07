//! Schema lifecycle delegated to the same generated contract as all networks.
use super::*;
use anyhow::ensure;
use serde_json::Value;
impl Database {
    pub async fn initialize(&self) -> Result<()> {
        // This HTTP driver selects its database on normal requests. Bootstrap
        // the namespace with a server-scoped request before using that driver.
        aml_warehouse::contract::identifier(self.database())?;
        self.request(
            &format!("CREATE DATABASE IF NOT EXISTS {}", self.database()),
            &[],
            None,
            false,
        )
        .await?;
        // All networks generate and apply the same current immutable phases.
        aml_warehouse::initialize(
            &SharedWarehouseConnection(self),
            self.database(),
            aml_warehouse::Network::Bitcoin,
        )
        .await?;
        crate::assets::initialize(self).await?;
        Ok(())
    }
    pub async fn ready(&self) -> Result<()> {
        let rows: Vec<Value> = self.rows("SELECT count() AS n FROM system.tables WHERE database = currentDatabase() AND name IN ('aml_sync_state_current','aml_ingested_blocks_canonical','aml_transactions_canonical','aml_bitcoin_outputs_canonical','aml_bitcoin_inputs_canonical','aml_address_relationships_canonical')", &[]).await?;
        ensure!(
            rows[0]["n"] == 6,
            "Bitcoin schema missing; run bitcoin_aml schema"
        );
        aml_warehouse::migration::validate_identity(
            &SharedWarehouseConnection(self),
            &self.config.database,
            aml_warehouse::Network::Bitcoin,
        )
        .await?;
        crate::assets::read(self).await?;
        Ok(())
    }
    pub async fn check_schema(&self) -> Result<aml_warehouse::SchemaSummary> {
        self.ready().await?;
        aml_warehouse::validate(
            &SharedWarehouseConnection(self),
            &self.config.database,
            aml_warehouse::Network::Bitcoin,
        )
        .await
    }
}

/// Shared by the dedicated schema binary and the compatible main CLI alias.
pub async fn schema_command(db: &Database, check: bool) -> Result<aml_warehouse::SchemaSummary> {
    if !check {
        crate::config::required("BITCOIN_WRITER_LOCK_DIR")?;
        let _owner = crate::writer_lock::WriterLock::for_database(db.database())?;
        db.initialize().await?;
        return db.check_schema().await;
    }
    db.check_schema().await
}
