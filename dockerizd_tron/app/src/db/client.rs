//! TRON publication connection and query-local core projections. Operational records move in DB-4.3.
use aml_warehouse::{Connection, Network};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Default)]
pub struct Client {
    pub(crate) inner: clickhouse::Client,
    database: String,
    #[cfg(test)]
    fail_point: std::sync::Arc<std::sync::atomic::AtomicU8>,
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
    pub fn from_inner(inner: clickhouse::Client, database: &str) -> Self {
        Self {
            inner: if database.is_empty() {
                inner
            } else {
                inner.with_database(database)
            },
            database: database.into(),
            #[cfg(test)]
            fail_point: Default::default(),
        }
    }
    pub fn database(&self) -> &str {
        &self.database
    }
    pub fn query(&self, sql: &str) -> clickhouse::query::Query {
        let query = self.inner.query(&super::read::expand(sql));
        let first = sql
            .trim_start()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if matches!(
            first.as_str(),
            "SELECT" | "WITH" | "SHOW" | "DESCRIBE" | "EXPLAIN"
        ) {
            query.with_option("readonly", "2")
        } else {
            query
        }
    }
    pub async fn revision(&self, table: &str) -> Result<u64> {
        aml_warehouse::contract::identifier(table)?;
        let highest = self
            .query(&format!(
                "SELECT toString(max(revision)) FROM {table} WHERE network_id='tron:mainnet'"
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
                .context("TRON row revision exhausted")?;
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
            "SELECT toJSONString(tuple({})) FROM {view} WHERE network_id='tron:mainnet' AND ({filter}) LIMIT 100001",
            aml_warehouse::core::projection(&schema.columns)
        );
        let rows = self.query(&sql).fetch_all::<String>().await?;
        ensure!(
            rows.len() <= 100000,
            "shared read exceeds the bounded 100000-row analysis scope"
        );
        aml_warehouse::core::decode_rows(table, Network::Tron, rows)
    }
    pub async fn invalidate_active(&self, family: &str) -> Result<()> {
        for p in self
            .rows_from(
                "aml_analysis_publications",
                "visible",
                &format!("family={}", aml_warehouse::publication::literal(family)),
            )
            .await?
        {
            aml_warehouse::visibility::invalidate_analysis(
                self,
                self.database(),
                Network::Tron,
                family,
                p["publication_id"]
                    .as_str()
                    .context("missing publication identity")?,
                self.revision("aml_analysis_publications").await?,
            )
            .await?;
        }
        Ok(())
    }
    pub async fn operational_version(&self) -> Result<Vec<String>> {
        let parts = [
            "aml_asset_metadata",
            "aml_metadata_jobs",
            "aml_intelligence_sources",
            "aml_intelligence_claims",
            "aml_intelligence_reviews",
            "aml_entities",
            "aml_entity_addresses",
            "aml_exposure_seeds",
            "aml_analysis_publications",
        ]
        .map(|table| {
            format!("(SELECT toString(max(revision)) FROM {table} WHERE network_id='tron:mainnet')")
        });
        let data = self
            .query(&format!("SELECT toJSONString(tuple({}))", parts.join(",")))
            .fetch_one::<String>()
            .await?;
        Ok(serde_json::from_str(&data)?)
    }
    pub async fn insert_rows(&self, table: &str, rows: &[Value]) -> Result<()> {
        aml_warehouse::publication::insert(self, self.database(), table, rows).await
    }
    pub fn with_option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner = self.inner.with_option(name, value);
        self
    }
    async fn read_strings(&self, sql: &str) -> Result<Vec<String>> {
        for attempt in 0..3 {
            let result = self
                .inner
                .query(&sql.replace('?', "??"))
                .with_option("readonly", "2")
                .fetch_all::<String>()
                .await;
            match result {
                Ok(rows) => return Ok(rows),
                Err(clickhouse::error::Error::Network(_)) if attempt < 2 => {
                    // fetch_all discards a partial response. Restart only a read;
                    // the caller's source fence still verifies snapshot consistency.
                    tokio::time::sleep(std::time::Duration::from_millis(50 << attempt)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("the last read attempt returns its error")
    }
}
impl Connection for Client {
    async fn execute(&self, sql: &str) -> Result<()> {
        #[cfg(test)]
        self.check_fault(sql, false)?;
        self.inner.query(&sql.replace('?', "??")).execute().await?;
        #[cfg(test)]
        self.check_fault(sql, true)?;
        Ok(())
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.read_strings(sql).await
    }
    async fn query_bounded(&self, sql: &str) -> Result<Vec<String>> {
        self.read_strings(sql).await
    }
}

#[cfg(test)]
impl Client {
    pub(crate) fn fail_once(&self, phase: u8) {
        self.fail_point.store(phase, Ordering::SeqCst);
    }
    fn check_fault(&self, sql: &str, after: bool) -> Result<()> {
        if !sql.starts_with("INSERT INTO ") {
            return Ok(());
        }
        let block = sql.contains(".aml_ingested_blocks FORMAT JSONEachRow");
        let reservation = block && sql.contains("\"ingestion_status\":\"processing\"");
        let complete = block && sql.contains("\"ingestion_status\":\"complete\"");
        let fact = aml_warehouse::core::FACTS
            .iter()
            .any(|t| sql.contains(&format!(".{t} FORMAT JSONEachRow")));
        let state = sql.contains(".aml_sync_state FORMAT JSONEachRow");
        let phase = match (after, reservation, fact, complete, state) {
            (false, true, _, _, _) => 1,
            (true, true, _, _, _) => 2,
            (true, _, true, _, _) => 3,
            (false, _, _, true, _) => 4,
            (true, _, _, true, _) => 5,
            (false, _, _, _, true) => 6,
            _ => 0,
        };
        if phase != 0
            && self
                .fail_point
                .compare_exchange(phase, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            anyhow::bail!("injected TRON publication interruption at phase {phase}");
        }
        Ok(())
    }
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
        .or_insert(json!(Network::Tron.id()));
    object.entry("revision").or_insert(json!(revision));
    object.entry("created_at_unix_ms").or_insert(json!(created));
    for defaults in [
        aml_warehouse::core_schema::defaults(table)?,
        aml_warehouse::analytics_schema::defaults(table)?,
        aml_warehouse::visibility_schema::defaults(table)?,
        aml_warehouse::tron_schema::defaults(table)?,
    ] {
        for (key, value) in defaults {
            object.entry(key).or_insert(value);
        }
    }
    aml_warehouse::semantics::validate_row(&aml_warehouse::current_contract()?, table, &data)?;
    Ok(data)
}

/// A read result is valid only against the same chain and operational dependencies.
pub struct ReadFence {
    source: aml_warehouse::visibility::Snapshot,
    operations: Vec<String>,
}
impl ReadFence {
    pub fn source_snapshot(&self) -> Value {
        self.source.value()
    }
    pub fn operational_snapshot(&self) -> Value {
        crate::db::records::operational_snapshot(&self.operations)
    }
    pub async fn capture(client: &Client) -> Result<Self> {
        Ok(Self {
            source: aml_warehouse::visibility::capture(client, client.database(), Network::Tron)
                .await?,
            operations: client.operational_version().await?,
        })
    }
    pub async fn unchanged(&self, client: &Client) -> Result<()> {
        aml_warehouse::visibility::unchanged(client, client.database(), &self.source).await?;
        ensure!(
            self.operations == client.operational_version().await?,
            "operational evidence changed; retry read"
        );
        Ok(())
    }
}

#[cfg(test)]
mod read_retry_tests {
    use super::*;
    use std::sync::{Arc, atomic::AtomicUsize};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Read the entire request before disconnecting: the client cannot assume the
    // server never received it. Every retried request must carry readonly=2.
    async fn server(
        replies: Vec<Option<(u16, &'static [u8])>>,
    ) -> Result<(
        Client,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<Result<()>>,
    )> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await?;
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 4096];
                    let size = socket.read(&mut bytes).await?;
                    ensure!(size > 0, "client closed before sending a complete request");
                    request.extend_from_slice(&bytes[..size]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .map(|s| s.trim().parse::<usize>())
                            .transpose()?
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            ensure!(
                                headers.contains("readonly=2"),
                                "retryable request was not read-only"
                            );
                            break;
                        }
                    }
                }
                observed.fetch_add(1, Ordering::SeqCst);
                if let Some((status, body)) = reply {
                    socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
                    socket.write_all(body).await?;
                    socket.shutdown().await?;
                }
            }
            Ok(())
        });
        let client = Client::from_inner(
            clickhouse::Client::default()
                .with_url(url)
                .with_compression(clickhouse::Compression::None)
                .with_validation(false),
            "",
        );
        Ok((client, calls, task))
    }

    #[tokio::test]
    async fn disconnected_read_restarts_and_returns_only_complete_rows() -> Result<()> {
        let (client, calls, task) = server(vec![None, Some((200, b"\x02ok"))]).await?;
        assert_eq!(Connection::query(&client, "SELECT 'ok'").await?, vec!["ok"]);
        task.await??;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn sql_errors_are_not_retried() -> Result<()> {
        let (client, calls, task) = server(vec![Some((400, b"missing column"))]).await?;
        let error = Connection::query_bounded(&client, "SELECT missing")
            .await
            .unwrap_err();
        assert!(
            error
                .downcast_ref::<clickhouse::error::Error>()
                .is_some_and(|e| matches!(e, clickhouse::error::Error::BadResponse(_)))
        );
        task.await??;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn transport_retries_stop_after_three_attempts() -> Result<()> {
        let (client, calls, task) = server(vec![None, None, None]).await?;
        let error = Connection::query(&client, "SELECT 'ok'").await.unwrap_err();
        assert!(
            error
                .downcast_ref::<clickhouse::error::Error>()
                .is_some_and(|e| matches!(e, clickhouse::error::Error::Network(_)))
        );
        task.await??;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        Ok(())
    }
}
