use std::{collections::HashMap, env, fs, path::Path, str::FromStr, sync::OnceLock};

#[derive(Debug, Clone)]
pub enum SyncMode {
    Backfill,
    Live,
    Auto,
}

impl SyncMode {
    fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "backfill" => Some(Self::Backfill),
            "live" => Some(Self::Live),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub sync_mode: SyncMode,
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_pass: String,
    pub clickhouse_db_tron: String,
    pub tron_rpc_url: Option<String>,
    pub tron_api_key: Option<String>,
    pub tron_start_block: u64,
    pub total_tron_txs: u64,
    pub tron_poll_interval_seconds: u64,
    pub tron_metadata_poll_interval_seconds: u64,
    pub tron_metadata_batch_size: u64,
    pub tron_metadata_max_attempts: u8,
    pub tron_protocol_registry_refresh_blocks: u64,
    pub tron_ingestion_batch_max_rows: usize,
    pub tron_ingestion_flush_interval_seconds: u64,
    pub rpc_timeout_seconds: u64,
    pub rpc_max_concurrency: usize,
    pub tx_worker_concurrency: usize,
    pub neo4j_uri: String,
    pub neo4j_username: String,
    pub neo4j_password: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let sync_mode = env_optional("SYNC_MODE")
            .and_then(|value| SyncMode::from_env_value(&value))
            .unwrap_or(SyncMode::Auto);

        Self {
            sync_mode,
            clickhouse_url: env_string("CLICKHOUSE_URL", "http://localhost:18123"),
            clickhouse_user: env_string("CLICKHOUSE_USER", "admin"),
            clickhouse_pass: env_string_any(&["CLICKHOUSE_PASSWORD", "CLICKHOUSE_PASS"], ""),
            clickhouse_db_tron: env_string("CLICKHOUSE_DB_TRON", "tron_db"),
            tron_rpc_url: env_optional_any(&["TRON_RPC_URL", "TRON_RPC_HTTP"])
                .or_else(|| Some("https://api.trongrid.io".to_string())),
            tron_api_key: env_optional_any(&["TRON_API_KEY", "TRONGRID_API_KEY"]),
            tron_start_block: env_parse("TRON_START_BLOCK", 0),
            total_tron_txs: env_parse("TOTAL_TRON_TXS", 200),
            tron_poll_interval_seconds: env_parse("TRON_POLL_INTERVAL_SECONDS", 3),
            tron_metadata_poll_interval_seconds: env_parse(
                "TRON_METADATA_POLL_INTERVAL_SECONDS",
                5,
            ),
            tron_metadata_batch_size: env_parse("TRON_METADATA_BATCH_SIZE", 100),
            tron_metadata_max_attempts: env_parse("TRON_METADATA_MAX_ATTEMPTS", 5),
            tron_protocol_registry_refresh_blocks: env_parse(
                "TRON_PROTOCOL_REGISTRY_REFRESH_BLOCKS",
                100,
            ),
            tron_ingestion_batch_max_rows: env_parse("TRON_INGESTION_BATCH_MAX_ROWS", 10_000),
            tron_ingestion_flush_interval_seconds: env_parse(
                "TRON_INGESTION_FLUSH_INTERVAL_SECONDS",
                120,
            ),
            rpc_timeout_seconds: env_parse("RPC_TIMEOUT_SECONDS", 120),
            rpc_max_concurrency: env_parse("RPC_MAX_CONCURRENCY", 2),
            tx_worker_concurrency: env_parse("TX_WORKER_CONCURRENCY", 2),
            neo4j_uri: env_string("NEO4J_URI", "localhost:17687"),
            neo4j_username: env_string("NEO4J_USERNAME", "neo4j"),
            neo4j_password: env_string("NEO4J_PASSWORD", ""),
        }
    }
}

fn env_string(key: &str, default: &str) -> String {
    env_optional(key).unwrap_or_else(|| default.to_string())
}

fn env_string_any(keys: &[&str], default: &str) -> String {
    env_optional_any(keys).unwrap_or_else(|| default.to_string())
}

fn env_optional_any(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| env_optional(key))
}

fn env_optional(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            local_env()
                .get(key)
                .cloned()
                .filter(|value| !value.trim().is_empty())
        })
}

fn local_env() -> &'static HashMap<String, String> {
    static LOCAL_ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

    LOCAL_ENV.get_or_init(|| {
        [Path::new(".env"), Path::new("app/.env")]
            .into_iter()
            .find_map(|path| fs::read_to_string(path).ok())
            .map(|contents| parse_env_file(&contents))
            .unwrap_or_default()
    })
}

fn parse_env_file(contents: &str) -> HashMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            let line = line.strip_prefix("export ").unwrap_or(line).trim();
            let (key, raw_value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty()
                || !key
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
            {
                return None;
            }

            let raw_value = raw_value.trim();
            let value = if raw_value.len() >= 2
                && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                    || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
            {
                &raw_value[1..raw_value.len() - 1]
            } else {
                raw_value
            };

            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn env_parse<T>(key: &str, default: T) -> T
where
    T: FromStr + Copy,
{
    env_optional(key)
        .and_then(|value| value.parse::<T>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::parse_env_file;

    #[test]
    fn parses_local_env_without_overwriting_rules() {
        let values = parse_env_file(
            r#"
            # local configuration
            TRON_API_KEY="secret-value"
            export NEO4J_PASSWORD='password'
            INVALID-KEY=ignored
            "#,
        );

        assert_eq!(
            values.get("TRON_API_KEY").map(String::as_str),
            Some("secret-value")
        );
        assert_eq!(
            values.get("NEO4J_PASSWORD").map(String::as_str),
            Some("password")
        );
        assert!(!values.contains_key("INVALID-KEY"));
    }
}
