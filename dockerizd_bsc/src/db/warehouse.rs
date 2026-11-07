use crate::config::ClickHouseConfig;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// Bounded JSON queries preserve UInt256 values as explicitly selected decimal strings.
#[derive(Clone)]
pub struct Warehouse {
    client: reqwest::Client,
    config: ClickHouseConfig,
    shared: super::Client,
}

impl Warehouse {
    pub fn new(config: ClickHouseConfig) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            shared: super::database_client(&config),
            config,
        })
    }

    pub(crate) async fn intelligence_snapshot(&self) -> Result<String> {
        Ok(self.shared.query("SELECT toJSONString(tuple((SELECT max(revision) FROM aml_intelligence_claims),(SELECT max(revision) FROM aml_intelligence_sources),(SELECT max(revision) FROM aml_intelligence_reviews)))").fetch_one::<String>().await?)
    }
    pub async fn snapshot(&self) -> Result<aml_warehouse::visibility::Snapshot> {
        aml_warehouse::visibility::capture(
            &self.shared,
            self.config.database(),
            aml_warehouse::Network::Bsc,
        )
        .await
    }
    pub async fn unchanged(&self, snapshot: &aml_warehouse::visibility::Snapshot) -> Result<()> {
        aml_warehouse::visibility::unchanged(&self.shared, self.config.database(), snapshot).await
    }
    pub async fn rows(&self, sql: &str, params: &[(&str, &str)]) -> Result<Vec<Value>> {
        let mut query = vec![
            ("database".to_owned(), self.config.database().to_owned()),
            ("readonly".into(), "2".into()),
            ("max_execution_time".into(), "15".into()),
            ("max_memory_usage".into(), "536870912".into()),
            ("max_result_rows".into(), "20000".into()),
            ("result_overflow_mode".into(), "throw".into()),
            ("output_format_json_quote_64bit_integers".into(), "0".into()),
        ];
        query.extend(
            params
                .iter()
                .map(|(name, value)| (format!("param_{name}"), (*value).to_owned())),
        );
        let mut response = self
            .client
            .post(self.config.endpoint())
            .header("X-ClickHouse-User", self.config.user())
            .header("X-ClickHouse-Key", self.config.password())
            .query(&query)
            .body(format!("{} FORMAT JSONEachRow", super::read::expand(sql)))
            .send()
            .await
            .context("warehouse request failed")?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                body.len() + chunk.len() <= 16 * 1024 * 1024,
                "warehouse response limit exceeded"
            );
            body.extend_from_slice(&chunk);
        }
        ensure!(
            status.is_success(),
            "warehouse query failed (HTTP {status}): {}",
            String::from_utf8_lossy(&body)
                .chars()
                .take(400)
                .collect::<String>()
        );
        std::str::from_utf8(&body)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).context("invalid warehouse result"))
            .collect()
    }

    pub async fn insert(&self, table: &str, rows: &[Value]) -> Result<()> {
        super::records::store(&self.shared, table, rows).await
    }
}
