//! Ethereum API projections over the shared physical schema. No native tables.
use crate::config::AppConfig;
use aml_warehouse::{Connection, Network};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Default)]
pub struct Client {
    pub(crate) inner: clickhouse::Client,
    database: String,
}
impl Client {
    pub fn with_url(mut self, value: impl Into<String>) -> Self {
        self.inner = self.inner.with_url(value);
        self
    }
    pub fn with_user(mut self, value: impl Into<String>) -> Self {
        self.inner = self.inner.with_user(value);
        self
    }
    pub fn with_password(mut self, value: impl Into<String>) -> Self {
        self.inner = self.inner.with_password(value);
        self
    }
    pub fn with_database(mut self, value: impl Into<String>) -> Self {
        self.database = value.into();
        self.inner = self.inner.with_database(&self.database);
        self
    }
    pub fn database(&self) -> &str {
        &self.database
    }
    pub fn query(&self, sql: &str) -> clickhouse::query::Query {
        self.inner
            .query(&super::read::expand(sql))
            .with_option("readonly", "2")
    }
    pub async fn revision(&self, table: &str) -> Result<u64> {
        aml_warehouse::contract::identifier(table)?;
        let highest = self
            .query(&format!(
                "SELECT toString(max(revision)) FROM {table} WHERE network_id='eip155:1'"
            ))
            .fetch_one::<String>()
            .await?
            .parse::<u64>()?;
        static LAST: AtomicU64 = AtomicU64::new(0);
        let clock = now_ms().saturating_mul(1000);
        let mut old = LAST.load(Ordering::SeqCst);
        loop {
            let next = old
                .max(highest)
                .max(clock)
                .checked_add(1)
                .context("Ethereum row revision exhausted")?;
            match LAST.compare_exchange(old, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return Ok(next),
                Err(v) => old = v,
            }
        }
    }
    pub async fn rows(&self, table: &str, filter: &str) -> Result<Vec<Value>> {
        self.rows_from(table, "current", filter).await
    }
    pub async fn rows_from(&self, table: &str, suffix: &str, filter: &str) -> Result<Vec<Value>> {
        let c = aml_warehouse::current_contract()?;
        let schema = c
            .tables
            .iter()
            .find(|t| t.name == table)
            .context("unsupported shared table")?;
        let view = format!("{table}_{suffix}");
        ensure!(
            c.views.iter().any(|v| v.name == view),
            "no current shared projection"
        );
        let sql = format!(
            "SELECT toJSONString(tuple({})) FROM {view} WHERE network_id='eip155:1' AND ({filter}) LIMIT 100001",
            aml_warehouse::core::projection(&schema.columns)
        );
        let rows = self.query(&sql).fetch_all::<String>().await?;
        ensure!(
            rows.len() <= 100000,
            "shared read exceeds the bounded 100000-row analysis scope"
        );
        aml_warehouse::core::decode_rows(table, Network::Ethereum, rows)
    }
    pub async fn invalidate_active(&self, family: &str) -> Result<()> {
        let entries = self
            .rows_from(
                "aml_analysis_publications",
                "visible",
                &format!("family={}", aml_warehouse::publication::literal(family)),
            )
            .await?;
        for p in entries {
            aml_warehouse::visibility::invalidate_analysis(
                self,
                self.database(),
                Network::Ethereum,
                family,
                p["publication_id"]
                    .as_str()
                    .context("publication identity missing")?,
                self.revision("aml_analysis_publications").await?,
            )
            .await?;
        }
        Ok(())
    }
    pub async fn insert_rows(&self, table: &str, rows: &[Value]) -> Result<()> {
        aml_warehouse::publication::insert(self, self.database(), table, rows).await
    }
}
impl Connection for Client {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.inner.query(&sql.replace('?', "??")).execute().await?;
        Ok(())
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        Ok(self
            .inner
            .query(&sql.replace('?', "??"))
            .fetch_all::<String>()
            .await?)
    }
    async fn query_bounded(&self, sql: &str) -> Result<Vec<String>> {
        Ok(self
            .inner
            .query(&sql.replace('?', "??"))
            .with_option("readonly", "2")
            .fetch_all::<String>()
            .await?)
    }
}
pub fn database_client(config: &AppConfig) -> Client {
    Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_password)
        .with_database(&config.clickhouse_database)
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn row(table: &str, mut data: Value, revision: u64, created: u64) -> Result<Value> {
    let object = data
        .as_object_mut()
        .context("shared row must be an object")?;
    object
        .entry("network_id")
        .or_insert(json!(Network::Ethereum.id()));
    object.entry("revision").or_insert(json!(revision));
    object.entry("created_at_unix_ms").or_insert(json!(created));
    for defaults in [
        aml_warehouse::core_schema::defaults(table)?,
        aml_warehouse::analytics_schema::defaults(table)?,
        aml_warehouse::visibility_schema::defaults(table)?,
    ] {
        for (key, value) in defaults {
            object.entry(key).or_insert(value);
        }
    }
    aml_warehouse::semantics::validate_row(&aml_warehouse::current_contract()?, table, &data)?;
    Ok(data)
}
pub fn refs(values: &Value) -> Vec<String> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|v| {
            if aml_warehouse::values::evidence_ref(Network::Ethereum, v).is_ok() {
                v.to_owned()
            } else if let Ok(hash) = aml_warehouse::values::normalize_hash(Network::Ethereum, v) {
                format!("tx:{hash}")
            } else {
                format!("source:{v}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn bare_evidence_hash_keeps_transaction_identity_after_normalization() {
        let upper = format!("0x{}", "AB".repeat(32));
        let values = super::refs(&serde_json::json!([
            upper,
            "case-7",
            "https://example.invalid/evidence"
        ]));
        assert_eq!(values[0], format!("tx:0x{}", "ab".repeat(32)));
        assert_eq!(values[1], "source:case-7");
        assert_eq!(values[2], "https://example.invalid/evidence");
        for value in values {
            aml_warehouse::values::evidence_ref(aml_warehouse::Network::Ethereum, &value).unwrap();
        }
    }
}
