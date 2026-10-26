//! HTTP transport and shared warehouse driver; no schema or domain SQL here.
use super::*;
use crate::config;
use anyhow::{Context, ensure};
use serde::de::DeserializeOwned;
use serde_json::Value;
impl aml_warehouse::Connection for SharedWarehouseConnection<'_> {
    async fn insert_json(&self, table: &str, body: &str) -> Result<()> {
        // Keep evidence in the HTTP body, never in a potentially oversized URL.
        self.0
            .request(
                &format!("INSERT INTO {table} FORMAT JSONEachRow"),
                &[],
                Some(body.into()),
                true,
            )
            .await?;
        Ok(())
    }
    async fn execute(&self, sql: &str) -> Result<()> {
        self.0.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.0
            .rows::<Value>(sql, &[])
            .await?
            .into_iter()
            .map(|row| {
                row.as_object()
                    .and_then(|v| v.values().next())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("shared warehouse expected a String column")
            })
            .collect()
    }
}
impl Database {
    pub(crate) fn warehouse_connection(&self) -> SharedWarehouseConnection<'_> {
        SharedWarehouseConnection(self)
    }
    pub(crate) fn database(&self) -> &str {
        &self.config.database
    }
    pub fn new(config: DatabaseConfig) -> Result<Self> {
        Ok(Self {
            client: config::http(90)?,
            config: Arc::new(config),
        })
    }
    pub(super) async fn request(
        &self,
        sql: &str,
        params: &[(&str, String)],
        body: Option<String>,
        database: bool,
    ) -> Result<String> {
        let mut query = vec![
            ("query".to_string(), sql.to_owned()),
            ("max_execution_time".into(), "60".into()),
            ("output_format_json_quote_64bit_integers".into(), "0".into()),
            ("async_insert".into(), "0".into()),
        ];
        if database {
            query.push(("database".into(), self.config.database.clone()));
        }
        for (key, value) in params {
            query.push((format!("param_{key}"), value.clone()));
        }
        let body = body.unwrap_or_default();
        let mut response = self
            .client
            .post(self.config.url.clone())
            .query(&query)
            .header("X-ClickHouse-User", &self.config.user)
            .header("X-ClickHouse-Key", &self.config.password)
            .header(reqwest::header::CONTENT_LENGTH, body.len())
            .body(body)
            .send()
            .await
            .context("ClickHouse transport failed")?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("ClickHouse response failed")?
        {
            ensure!(
                bytes.len() + chunk.len() <= 32 * 1024 * 1024,
                "ClickHouse response safety limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(bytes)?;
        ensure!(
            status.is_success(),
            "ClickHouse error: {}",
            text.chars().take(1000).collect::<String>()
        );
        Ok(text)
    }
    pub async fn execute(&self, sql: &str) -> Result<()> {
        self.request(sql, &[], None, true).await?;
        Ok(())
    }
    pub async fn rows<T: DeserializeOwned>(
        &self,
        sql: &str,
        params: &[(&str, String)],
    ) -> Result<Vec<T>> {
        let text = self
            .request(&format!("{sql} FORMAT JSONEachRow"), params, None, true)
            .await?;
        text.lines()
            .filter(|v| !v.trim().is_empty())
            .map(|s| serde_json::from_str(s).context("invalid ClickHouse row"))
            .collect()
    }
    pub async fn insert<T: Serialize>(&self, table: &str, rows: &[T]) -> Result<()> {
        ensure!(
            [
                "bitcoin_transactions",
                "bitcoin_inputs",
                "bitcoin_outputs",
                "bitcoin_block_commits",
                "bitcoin_sync_state"
            ]
            .contains(&table),
            "unsupported table"
        );
        // Bound each HTTP insert; a commit is published only after all chunks succeed.
        for chunk in rows.chunks(1000) {
            let mut body = String::new();
            for row in chunk {
                body.push_str(&serde_json::to_string(row)?);
                body.push('\n');
            }
            self.request(
                &format!("INSERT INTO {table} FORMAT JSONEachRow"),
                &[],
                Some(body),
                true,
            )
            .await?;
        }
        Ok(())
    }
}
