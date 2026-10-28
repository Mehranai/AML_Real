//! The same versioned warehouse contract is embedded by all four adapters.
//! Legacy objects are deliberately outside this namespace during migration.
pub mod analytics;
pub mod analytics_schema;
pub mod contract;
pub mod core;
pub mod core_schema;
pub mod migration;
pub mod publication;
pub mod relationships;
pub mod semantics;
pub mod types;
pub mod values;
pub mod visibility;
pub mod visibility_schema;

pub use contract::{
    Contract, analytics_contract, baseline_contract, column_contract, core_contract,
    current_contract,
};
pub use migration::{Connection, SchemaSummary, initialize, validate};
pub use types::{Network, RawAmount, UnixMillis};

/// Adapter for any of the clickhouse client versions already used by this repo.
/// Queries return a single JSON string column so native Row decoding stays local
/// to the adapter's clickhouse version instead of adding a second driver.
#[macro_export]
macro_rules! clickhouse_connection {
    ($name:ident) => {
        struct $name<'a>(&'a clickhouse::Client);
        impl $crate::Connection for $name<'_> {
            async fn execute(&self, sql: &str) -> anyhow::Result<()> {
                // Connection receives fully rendered SQL. Native clickhouse
                // clients bind '?' even inside literals; '??' preserves it.
                self.0.query(&sql.replace('?', "??")).execute().await?;
                Ok(())
            }
            async fn query(&self, sql: &str) -> anyhow::Result<Vec<String>> {
                Ok(self
                    .0
                    .query(&sql.replace('?', "??"))
                    .fetch_all::<String>()
                    .await?)
            }
            async fn query_bounded(&self, sql: &str) -> anyhow::Result<Vec<String>> {
                // readonly=2 still forbids writes but permits query limits.
                // Full schema --check continues to use query() unchanged.
                Ok(self
                    .0
                    .query(&sql.replace('?', "??"))
                    .with_option("readonly", "2")
                    .fetch_all::<String>()
                    .await?)
            }
        }
    };
}
