use std::{collections::HashMap, env, fs, path::Path, str::FromStr, sync::OnceLock};

use anyhow::{Context, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalityTag {
    Finalized,
    Safe,
}

impl FinalityTag {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "finalized" => Some(Self::Finalized),
            "safe" => Some(Self::Safe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceMode {
    Disabled,
    Auto,
    Required,
}

impl TraceMode {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" => Ok(Self::Disabled),
            "auto" => Ok(Self::Auto),
            "required" | "require" => Ok(Self::Required),
            _ => anyhow::bail!("ETH_TRACE_MODE must be disabled, auto, or required"),
        }
    }

    pub fn enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn required(self) -> bool {
        matches!(self, Self::Required)
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub eth_rpc_url: String,
    pub eth_expected_chain_id: u64,
    pub eth_network_id: String,
    pub eth_start_block: u64,
    pub eth_finality_tag: FinalityTag,
    pub eth_poll_interval_seconds: u64,
    pub eth_ingestion_batch_max_rows: usize,
    pub eth_rpc_provider: String,
    pub eth_rpc_max_retries: u32,
    pub eth_rpc_retry_base_delay_ms: u64,
    pub eth_request_delay_ms: u64,
    pub eth_trace_mode: TraceMode,
    pub eth_protocol_registry_refresh_blocks: u64,
    pub eth_token_metadata_poll_seconds: u64,
    pub eth_token_metadata_batch_size: u64,
    pub eth_token_metadata_max_attempts: u8,
    pub eth_risk_engine_enabled: bool,
    pub eth_risk_policy_version: String,
    pub eth_analytics_interval_seconds: u64,
    pub eth_exposure_max_hops: u8,
    pub eth_exposure_hop_decay: f64,
    pub eth_exposure_time_half_life_days: f64,
    pub eth_exposure_max_paths_per_subject: u16,
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub clickhouse_database: String,
    pub neo4j_uri: String,
    pub neo4j_username: String,
    pub neo4j_password: String,
    pub ethereum_api_addr: String,
    pub eth_graph_max_edges: usize,
    pub rpc_timeout_seconds: u64,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            eth_rpc_url: env_string("ETH_RPC_URL", "http://127.0.0.1:8545"),
            eth_expected_chain_id: env_parse("ETH_EXPECTED_CHAIN_ID", 1_u64)?,
            eth_network_id: env_string("ETH_NETWORK_ID", "eip155:1"),
            eth_start_block: env_parse("ETH_START_BLOCK", 0_u64)?,
            eth_finality_tag: env_optional("ETH_FINALITY_TAG")
                .as_deref()
                .and_then(FinalityTag::parse)
                .unwrap_or(FinalityTag::Finalized),
            eth_poll_interval_seconds: env_parse("ETH_POLL_INTERVAL_SECONDS", 12_u64)?,
            eth_ingestion_batch_max_rows: env_parse("ETH_INGESTION_BATCH_MAX_ROWS", 10_000_usize)?,
            eth_rpc_provider: env_string("ETH_RPC_PROVIDER", "custom"),
            eth_rpc_max_retries: env_parse("ETH_RPC_MAX_RETRIES", 5_u32)?,
            eth_rpc_retry_base_delay_ms: env_parse("ETH_RPC_RETRY_BASE_DELAY_MS", 500_u64)?,
            eth_request_delay_ms: env_parse("ETH_REQUEST_DELAY_MS", 100_u64)?,
            eth_trace_mode: TraceMode::parse(&env_string("ETH_TRACE_MODE", "auto"))?,
            eth_protocol_registry_refresh_blocks: env_parse(
                "ETH_PROTOCOL_REGISTRY_REFRESH_BLOCKS",
                100_u64,
            )?,
            eth_token_metadata_poll_seconds: env_parse("ETH_TOKEN_METADATA_POLL_SECONDS", 30_u64)?,
            eth_token_metadata_batch_size: env_parse("ETH_TOKEN_METADATA_BATCH_SIZE", 100_u64)?,
            eth_token_metadata_max_attempts: env_parse("ETH_TOKEN_METADATA_MAX_ATTEMPTS", 5_u8)?,
            eth_risk_engine_enabled: env_parse("ETH_RISK_ENGINE_ENABLED", true)?,
            eth_risk_policy_version: env_string(
                "ETH_RISK_POLICY_VERSION",
                "ethereum_evidence_policy_v1",
            ),
            eth_analytics_interval_seconds: env_parse(
                "ETH_ANALYTICS_INTERVAL_SECONDS",
                86_400_u64,
            )?,
            eth_exposure_max_hops: env_parse("ETH_EXPOSURE_MAX_HOPS", 5_u8)?,
            eth_exposure_hop_decay: env_parse("ETH_EXPOSURE_HOP_DECAY", 0.65_f64)?,
            eth_exposure_time_half_life_days: env_parse(
                "ETH_EXPOSURE_TIME_HALF_LIFE_DAYS",
                365.0_f64,
            )?,
            eth_exposure_max_paths_per_subject: env_parse(
                "ETH_EXPOSURE_MAX_PATHS_PER_SUBJECT",
                3_u16,
            )?,
            clickhouse_url: env_string("CLICKHOUSE_URL", "http://127.0.0.1:8123"),
            clickhouse_user: env_string("CLICKHOUSE_USER", "default"),
            clickhouse_password: env_string("CLICKHOUSE_PASSWORD", ""),
            clickhouse_database: env_string("CLICKHOUSE_DB_ETH", "ethereum_aml"),
            neo4j_uri: env_string("NEO4J_URI", "127.0.0.1:7687"),
            neo4j_username: env_string("NEO4J_USERNAME", "neo4j"),
            neo4j_password: env_string("NEO4J_PASSWORD", ""),
            ethereum_api_addr: env_string("ETHEREUM_API_ADDR", "127.0.0.1:5001"),
            eth_graph_max_edges: env_parse("ETH_GRAPH_MAX_EDGES", 20_000_usize)?,
            rpc_timeout_seconds: env_parse("RPC_TIMEOUT_SECONDS", 120_u64)?,
        };

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.eth_expected_chain_id > 0,
            "ETH_EXPECTED_CHAIN_ID must be positive"
        );
        ensure!(
            self.eth_network_id == format!("eip155:{}", self.eth_expected_chain_id),
            "ETH_NETWORK_ID must match ETH_EXPECTED_CHAIN_ID"
        );
        ensure!(
            self.eth_ingestion_batch_max_rows > 0,
            "ETH_INGESTION_BATCH_MAX_ROWS must be positive"
        );
        ensure!(
            self.eth_rpc_max_retries > 0,
            "ETH_RPC_MAX_RETRIES must be positive"
        );
        ensure!(
            self.eth_protocol_registry_refresh_blocks > 0,
            "ETH_PROTOCOL_REGISTRY_REFRESH_BLOCKS must be positive"
        );
        ensure!(
            self.eth_token_metadata_poll_seconds > 0,
            "ETH_TOKEN_METADATA_POLL_SECONDS must be positive"
        );
        ensure!(
            self.eth_token_metadata_batch_size > 0,
            "ETH_TOKEN_METADATA_BATCH_SIZE must be positive"
        );
        ensure!(
            self.eth_token_metadata_max_attempts > 0,
            "ETH_TOKEN_METADATA_MAX_ATTEMPTS must be positive"
        );
        ensure!(
            !self.eth_risk_policy_version.trim().is_empty(),
            "ETH_RISK_POLICY_VERSION must not be empty"
        );
        ensure!(
            self.eth_analytics_interval_seconds > 0,
            "ETH_ANALYTICS_INTERVAL_SECONDS must be positive"
        );
        ensure!(
            (1..=10).contains(&self.eth_exposure_max_hops),
            "ETH_EXPOSURE_MAX_HOPS must be 1..=10"
        );
        ensure!(
            self.eth_exposure_hop_decay > 0.0 && self.eth_exposure_hop_decay <= 1.0,
            "ETH_EXPOSURE_HOP_DECAY must be greater than 0 and at most 1"
        );
        ensure!(
            self.eth_exposure_time_half_life_days > 0.0,
            "ETH_EXPOSURE_TIME_HALF_LIFE_DAYS must be positive"
        );
        ensure!(
            (1..=25).contains(&self.eth_exposure_max_paths_per_subject),
            "ETH_EXPOSURE_MAX_PATHS_PER_SUBJECT must be 1..=25"
        );
        ensure!(
            !self.eth_rpc_provider.trim().is_empty(),
            "ETH_RPC_PROVIDER must not be empty"
        );
        ensure!(
            is_safe_clickhouse_identifier(&self.clickhouse_database),
            "CLICKHOUSE_DB_ETH contains an unsafe identifier"
        );
        ensure!(
            self.eth_graph_max_edges > 0,
            "ETH_GRAPH_MAX_EDGES must be positive"
        );
        ensure!(
            !self.ethereum_api_addr.trim().is_empty(),
            "ETHEREUM_API_ADDR must not be empty"
        );

        Ok(())
    }
}

fn is_safe_clickhouse_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn env_string(key: &str, default: &str) -> String {
    env_optional(key).unwrap_or_else(|| default.to_string())
}

fn env_parse<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env_optional(key) {
        Some(value) => value
            .parse::<T>()
            .with_context(|| format!("invalid value for {key}")),
        None => Ok(default),
    }
}

fn env_optional(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| local_env().get(key).cloned())
}

fn local_env() -> &'static HashMap<String, String> {
    static LOCAL_ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

    LOCAL_ENV.get_or_init(|| {
        Path::new(".env")
            .is_file()
            .then(|| fs::read_to_string(".env").ok())
            .flatten()
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
            if !is_safe_clickhouse_identifier(key) {
                return None;
            }

            let raw_value = raw_value.trim();
            let value = if raw_value.len() >= 2
                && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                    || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
            {
                raw_value[1..raw_value.len() - 1].to_string()
            } else {
                raw_value.to_string()
            };

            Some((key.to_string(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{is_safe_clickhouse_identifier, parse_env_file};

    #[test]
    fn parses_local_env_values() {
        let parsed = parse_env_file(
            r#"
            ETH_RPC_URL="http://127.0.0.1:8545"
            export CLICKHOUSE_DB_ETH='eth_db'
            INVALID-KEY=ignored
            "#,
        );

        assert_eq!(
            parsed.get("ETH_RPC_URL").map(String::as_str),
            Some("http://127.0.0.1:8545")
        );
        assert_eq!(
            parsed.get("CLICKHOUSE_DB_ETH").map(String::as_str),
            Some("eth_db")
        );
        assert!(!parsed.contains_key("INVALID-KEY"));
    }

    #[test]
    fn validates_clickhouse_identifiers() {
        assert!(is_safe_clickhouse_identifier("eth_db"));
        assert!(!is_safe_clickhouse_identifier(
            "eth_db; DROP DATABASE eth_db"
        ));
    }
}
