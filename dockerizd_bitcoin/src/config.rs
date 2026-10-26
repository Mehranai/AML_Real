use anyhow::{Context, Result, ensure};
use std::{env, time::Duration};

pub fn setting(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}
pub fn bounded(key: &str, default: u64, min: u64, max: u64) -> Result<u64> {
    let value = setting(key, &default.to_string())
        .parse::<u64>()
        .with_context(|| format!("invalid {key}"))?;
    ensure!((min..=max).contains(&value), "{key} must be {min}..{max}");
    Ok(value)
}
pub fn required(key: &str) -> Result<String> {
    env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .with_context(|| format!("{key} is required"))
}
pub fn origin(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).context("invalid service URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "service URL must be an HTTP(S) origin; use separate credential settings"
    );
    Ok(url)
}
pub fn http(timeout: u64) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

#[derive(Clone)]
pub struct DatabaseConfig {
    pub url: reqwest::Url,
    pub user: String,
    pub password: String,
    pub database: String,
}
impl DatabaseConfig {
    pub fn from_env() -> Result<Self> {
        let database = setting("BITCOIN_CLICKHOUSE_DATABASE", "bitcoin_aml");
        ensure!(
            database == "bitcoin_aml",
            "BITCOIN_CLICKHOUSE_DATABASE must be bitcoin_aml (chain isolation)"
        );
        Ok(Self {
            url: origin(&setting("BITCOIN_CLICKHOUSE_URL", "http://127.0.0.1:48123"))?,
            user: setting("BITCOIN_CLICKHOUSE_USER", "bitcoin_admin"),
            password: required("BITCOIN_CLICKHOUSE_PASSWORD")?,
            database,
        })
    }
}

#[derive(Clone, Debug)]
pub struct IngestConfig {
    pub start: u64,
    pub end: Option<u64>,
    pub confirmations: u64,
    pub reorg_depth: u64,
    pub poll_seconds: u64,
    pub verbosity: u64,
}
impl IngestConfig {
    pub fn from_env() -> Result<Self> {
        let start = bounded("BITCOIN_START_BLOCK", 0, 0, u32::MAX as u64)?;
        let end = env::var("BITCOIN_END_BLOCK")
            .ok()
            .filter(|v| !v.is_empty())
            .map(|v| v.parse::<u64>())
            .transpose()
            .context("invalid BITCOIN_END_BLOCK")?;
        ensure!(
            end.is_none_or(|v| v >= start && v <= u32::MAX as u64),
            "invalid Bitcoin block range"
        );
        Ok(Self {
            start,
            end,
            confirmations: bounded("BITCOIN_CONFIRMATIONS", 6, 1, 1000)?,
            reorg_depth: bounded("BITCOIN_REORG_MAX_DEPTH", 144, 1, 10000)?,
            poll_seconds: bounded("BITCOIN_POLL_SECONDS", 10, 1, 3600)?,
            verbosity: bounded("BITCOIN_BLOCK_VERBOSITY", 3, 2, 3)?,
        })
    }
}

#[derive(Clone, Debug)]
pub struct RpcPolicy {
    pub attempts: u64,
    pub retry_base_ms: u64,
    pub interval_ms: u64,
    pub max_response_bytes: usize,
    pub batch_enabled: bool,
}
impl Default for RpcPolicy {
    fn default() -> Self {
        Self {
            attempts: 1,
            retry_base_ms: 0,
            interval_ms: 0,
            max_response_bytes: 128 * 1024 * 1024,
            batch_enabled: true,
        }
    }
}
impl RpcPolicy {
    pub fn from_env() -> Result<Self> {
        let batch_enabled = match setting("BITCOIN_RPC_BATCH_ENABLED", "true").as_str() {
            "true" => true,
            "false" => false,
            _ => anyhow::bail!("BITCOIN_RPC_BATCH_ENABLED must be true or false"),
        };
        Ok(Self {
            attempts: bounded("BITCOIN_RPC_MAX_ATTEMPTS", 4, 1, 8)?,
            retry_base_ms: bounded("BITCOIN_RPC_RETRY_BASE_MS", 250, 0, 5000)?,
            interval_ms: bounded("BITCOIN_RPC_REQUEST_INTERVAL_MS", 50, 0, 10000)?,
            max_response_bytes: bounded(
                "BITCOIN_RPC_MAX_RESPONSE_BYTES",
                64 * 1024 * 1024,
                1024,
                128 * 1024 * 1024,
            )? as usize,
            batch_enabled,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PipelineConfig {
    pub block_batch: usize,
    pub block_concurrency: usize,
    pub prevout_batch: usize,
    pub prevout_concurrency: usize,
    pub max_prevouts: usize,
    pub max_resolved_bytes: usize,
    pub fallback_cooldown_seconds: u64,
}
impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            block_batch: 2,
            block_concurrency: 2,
            prevout_batch: 128,
            prevout_concurrency: 2,
            max_prevouts: 100_000,
            max_resolved_bytes: 64 * 1024 * 1024,
            fallback_cooldown_seconds: 30,
        }
    }
}
impl PipelineConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            block_batch: bounded("BITCOIN_BLOCK_BATCH_SIZE", 2, 1, 8)? as usize,
            block_concurrency: bounded("BITCOIN_BLOCK_FETCH_CONCURRENCY", 2, 1, 4)? as usize,
            prevout_batch: bounded("BITCOIN_PREVOUT_BATCH_SIZE", 128, 1, 256)? as usize,
            prevout_concurrency: bounded("BITCOIN_PREVOUT_CONCURRENCY", 2, 1, 4)? as usize,
            max_prevouts: bounded("BITCOIN_MAX_PREVOUTS_PER_BLOCK", 100_000, 1, 500_000)? as usize,
            max_resolved_bytes: bounded(
                "BITCOIN_MAX_RESOLVED_BYTES",
                64 * 1024 * 1024,
                1024,
                128 * 1024 * 1024,
            )? as usize,
            fallback_cooldown_seconds: bounded("BITCOIN_FALLBACK_COOLDOWN_SECONDS", 30, 1, 600)?,
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=8).contains(&self.block_batch)
                && (1..=4).contains(&self.block_concurrency)
                && (1..=256).contains(&self.prevout_batch)
                && (1..=4).contains(&self.prevout_concurrency)
                && (1..=500_000).contains(&self.max_prevouts)
                && (1024..=128 * 1024 * 1024).contains(&self.max_resolved_bytes)
                && (1..=600).contains(&self.fallback_cooldown_seconds),
            "invalid Bitcoin pipeline bounds"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_and_paths_cannot_be_embedded_in_origins() {
        for value in [
            "file:///tmp/rpc",
            "http://user:pass@host",
            "http://host/?key=secret",
            "http://host/wallet/x",
        ] {
            assert!(origin(value).is_err());
        }
        assert!(origin("http://127.0.0.1:8332").is_ok());
    }
}
