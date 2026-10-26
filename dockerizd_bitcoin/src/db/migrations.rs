//! Schema lifecycle; embeds the active, generated SQL migration.
use super::*;
use anyhow::ensure;
use serde_json::Value;
pub const BASELINE_SQL: &str =
    include_str!("../../sql/migrations/20261022_0001_shared_warehouse.sql");
pub const COLUMN_SQL: &str = include_str!("../../sql/migrations/20261026_0002_column_contract.sql");
impl Database {
    pub async fn initialize(&self) -> Result<()> {
        let sql = super::sql::baseline_sql()?;
        let columns = super::sql::column_sql()?;
        // New installs create only the shared contract. Existing legacy objects
        // are left intact for the explicit, resumable backfill command.
        aml_warehouse::contract::identifier(self.database())?;
        self.request(
            &format!("CREATE DATABASE IF NOT EXISTS {}", self.database()),
            &[],
            None,
            false,
        )
        .await?;
        aml_warehouse::migration::initialize_from_migrations(
            &SharedWarehouseConnection(self),
            &self.config.database,
            aml_warehouse::Network::Bitcoin,
            &sql,
            &columns,
        )
        .await?;
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

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_migration_is_the_shared_contract_not_an_independent_schema() {
        let contract = aml_warehouse::baseline_contract().unwrap();
        assert_eq!(
            contract
                .statements_from_template("bitcoin_aml", super::BASELINE_SQL)
                .unwrap(),
            contract.statements("bitcoin_aml").unwrap()
        );
        assert!(
            contract
                .statements_from_template(
                    "bitcoin_aml",
                    &super::BASELINE_SQL.replace("UInt256", "UInt64")
                )
                .is_err()
        );
        assert_eq!(
            super::COLUMN_SQL.replace("\r\n", "\n"),
            aml_warehouse::semantics::upgrade_template().unwrap()
        );
        assert!(
            aml_warehouse::semantics::upgrade_statements(
                "bitcoin_aml",
                &super::COLUMN_SQL.replace("aml_columns_v2", "unreviewed_constraint")
            )
            .is_err()
        );
        assert!(
            contract
                .statements_from_template("bitcoin_aml; DROP DATABASE x", super::BASELINE_SQL)
                .is_err()
        );
    }
}
