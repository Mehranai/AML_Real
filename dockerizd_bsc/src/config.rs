use std::{collections::HashMap, env, fmt, fs, path::Path, time::Duration};

use reqwest::Url;

use crate::BSC_CLICKHOUSE_DATABASE;

pub fn setting(key: &str) -> Result<Option<String>, ConfigError> {
    let local = load_env_file(Path::new(".env"))?;
    Ok(env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| local.get(key).cloned().filter(|v| !v.trim().is_empty())))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    Development,
    Production,
}

impl DeploymentMode {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" => Ok(Self::Development),
            "production" | "prod" => Ok(Self::Production),
            _ => Err(ConfigError::InvalidValue {
                key: "BSC_MODE",
                reason: "must be development or production",
            }),
        }
    }
}

impl fmt::Display for DeploymentMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Development => "development",
            Self::Production => "production",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceMode {
    Disabled,
    Auto,
    Required,
}

impl TraceMode {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" => Ok(Self::Disabled),
            "auto" | "optional" => Ok(Self::Auto),
            "required" => Ok(Self::Required),
            _ => Err(ConfigError::InvalidValue {
                key: "BSC_TRACE_MODE",
                reason: "must be disabled, auto, or required",
            }),
        }
    }

    pub fn enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn required(self) -> bool {
        matches!(self, Self::Required)
    }
}

impl fmt::Display for TraceMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Disabled => "disabled",
            Self::Auto => "auto",
            Self::Required => "required",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptFetchMode {
    Auto,
    Block,
    Transaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Range,
    Auto,
    Follow,
}

impl SyncMode {
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "range" | "historical" => Ok(Self::Range),
            "auto" | "resume" => Ok(Self::Auto),
            "follow" | "live" => Ok(Self::Follow),
            _ => Err(ConfigError::InvalidValue {
                key: "BSC_SYNC_MODE",
                reason: "must be range, auto, or follow",
            }),
        }
    }
}

impl fmt::Display for SyncMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Range => "range",
            Self::Auto => "auto",
            Self::Follow => "follow",
        })
    }
}

impl ReceiptFetchMode {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "block" | "block_receipts" => Ok(Self::Block),
            "transaction" | "per_transaction" => Ok(Self::Transaction),
            _ => Err(ConfigError::InvalidValue {
                key: "BSC_RECEIPT_FETCH_MODE",
                reason: "must be auto, block, or transaction",
            }),
        }
    }
}

impl fmt::Display for ReceiptFetchMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Block => "block",
            Self::Transaction => "transaction",
        })
    }
}

#[derive(Debug, Clone)]
pub struct IngestionConfig {
    pub sync_mode: SyncMode,
    pub receipt_fetch_mode: ReceiptFetchMode,
    pub block_fetch_concurrency: usize,
    pub receipt_concurrency: usize,
    pub rpc_max_attempts: u32,
    pub retry_base_delay: Duration,
    pub max_response_bytes: usize,
    pub max_blocks_per_run: u64,
    pub batch_size: u64,
    pub poll_interval: Duration,
    pub request_delay: Duration,
    pub reorg_max_depth: u64,
    pub failure_max_attempts: u32,
    pub repair_max_blocks: u64,
    pub protocol_registry_refresh_blocks: u64,
    pub start_block: Option<u64>,
    pub end_block: Option<u64>,
}

impl IngestionConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let local = load_env_file(Path::new(".env"))?;
        Self::from_lookup(|key| {
            env::var(key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| local.get(key).cloned().filter(|value| !value.is_empty()))
        })
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let sync_mode = SyncMode::parse(lookup("BSC_SYNC_MODE").as_deref().unwrap_or("auto"))?;
        let receipt_fetch_mode = ReceiptFetchMode::parse(
            lookup("BSC_RECEIPT_FETCH_MODE")
                .as_deref()
                .unwrap_or("auto"),
        )?;
        let block_fetch_concurrency = parse_number(
            "BSC_BLOCK_FETCH_CONCURRENCY",
            lookup("BSC_BLOCK_FETCH_CONCURRENCY"),
            8_usize,
        )?;
        if !(1..=32).contains(&block_fetch_concurrency) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_BLOCK_FETCH_CONCURRENCY",
                reason: "must be between 1 and 32",
            });
        }
        let receipt_concurrency = parse_number(
            "BSC_RECEIPT_CONCURRENCY",
            lookup("BSC_RECEIPT_CONCURRENCY"),
            8_usize,
        )?;
        if !(1..=64).contains(&receipt_concurrency) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RECEIPT_CONCURRENCY",
                reason: "must be between 1 and 64",
            });
        }
        let rpc_max_attempts = parse_number(
            "BSC_RPC_MAX_ATTEMPTS",
            lookup("BSC_RPC_MAX_ATTEMPTS"),
            4_u32,
        )?;
        if !(1..=10).contains(&rpc_max_attempts) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_MAX_ATTEMPTS",
                reason: "must be between 1 and 10",
            });
        }
        let retry_base_millis = parse_number(
            "BSC_RPC_RETRY_BASE_MILLIS",
            lookup("BSC_RPC_RETRY_BASE_MILLIS"),
            250_u64,
        )?;
        if !(10..=10_000).contains(&retry_base_millis) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_RETRY_BASE_MILLIS",
                reason: "must be between 10 and 10000",
            });
        }
        let max_response_bytes = parse_number(
            "BSC_RPC_MAX_RESPONSE_BYTES",
            lookup("BSC_RPC_MAX_RESPONSE_BYTES"),
            64_usize * 1024 * 1024,
        )?;
        if !(1_048_576..=268_435_456).contains(&max_response_bytes) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_MAX_RESPONSE_BYTES",
                reason: "must be between 1 MiB and 256 MiB",
            });
        }
        let max_blocks_per_run = parse_number(
            "BSC_INGEST_MAX_BLOCKS_PER_RUN",
            lookup("BSC_INGEST_MAX_BLOCKS_PER_RUN"),
            1_000_u64,
        )?;
        if !(1..=100_000).contains(&max_blocks_per_run) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_INGEST_MAX_BLOCKS_PER_RUN",
                reason: "must be between 1 and 100000",
            });
        }
        let batch_size = parse_number(
            "BSC_INGEST_BATCH_SIZE",
            lookup("BSC_INGEST_BATCH_SIZE"),
            100_u64,
        )?;
        if !(1..=1_000).contains(&batch_size) || batch_size > max_blocks_per_run {
            return Err(ConfigError::InvalidValue {
                key: "BSC_INGEST_BATCH_SIZE",
                reason: "must be between 1 and 1000 and not exceed max blocks per run",
            });
        }
        let poll_interval_seconds = parse_number(
            "BSC_POLL_INTERVAL_SECONDS",
            lookup("BSC_POLL_INTERVAL_SECONDS"),
            3_u64,
        )?;
        if !(1..=300).contains(&poll_interval_seconds) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_POLL_INTERVAL_SECONDS",
                reason: "must be between 1 and 300",
            });
        }
        let request_delay_millis = parse_number(
            "BSC_RPC_REQUEST_DELAY_MILLIS",
            lookup("BSC_RPC_REQUEST_DELAY_MILLIS"),
            0_u64,
        )?;
        if request_delay_millis > 60_000 {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_REQUEST_DELAY_MILLIS",
                reason: "must be at most 60000",
            });
        }
        let reorg_max_depth = parse_number(
            "BSC_REORG_MAX_DEPTH",
            lookup("BSC_REORG_MAX_DEPTH"),
            128_u64,
        )?;
        if !(1..=2_048).contains(&reorg_max_depth) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_REORG_MAX_DEPTH",
                reason: "must be between 1 and 2048",
            });
        }
        let failure_max_attempts = parse_number(
            "BSC_FAILURE_MAX_ATTEMPTS",
            lookup("BSC_FAILURE_MAX_ATTEMPTS"),
            10_u32,
        )?;
        if !(1..=100).contains(&failure_max_attempts) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_FAILURE_MAX_ATTEMPTS",
                reason: "must be between 1 and 100",
            });
        }
        let repair_max_blocks = parse_number(
            "BSC_REPAIR_MAX_BLOCKS",
            lookup("BSC_REPAIR_MAX_BLOCKS"),
            1_000_u64,
        )?;
        if !(1..=10_000).contains(&repair_max_blocks) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_REPAIR_MAX_BLOCKS",
                reason: "must be between 1 and 10000",
            });
        }
        let protocol_registry_refresh_blocks = parse_number(
            "BSC_PROTOCOL_REGISTRY_REFRESH_BLOCKS",
            lookup("BSC_PROTOCOL_REGISTRY_REFRESH_BLOCKS"),
            1_000_u64,
        )?;
        if !(1..=100_000).contains(&protocol_registry_refresh_blocks) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_PROTOCOL_REGISTRY_REFRESH_BLOCKS",
                reason: "must be between 1 and 100000",
            });
        }
        let start_block =
            parse_optional_number("BSC_INGEST_START_BLOCK", lookup("BSC_INGEST_START_BLOCK"))?;
        let end_block =
            parse_optional_number("BSC_INGEST_END_BLOCK", lookup("BSC_INGEST_END_BLOCK"))?;
        if start_block.is_none() && end_block.is_some() {
            return Err(ConfigError::InvalidValue {
                key: "BSC_INGEST_END_BLOCK",
                reason: "requires BSC_INGEST_START_BLOCK",
            });
        }
        if matches!((start_block, end_block), (Some(start), Some(end)) if start > end) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_INGEST_END_BLOCK",
                reason: "must be greater than or equal to the start block",
            });
        }

        Ok(Self {
            sync_mode,
            receipt_fetch_mode,
            block_fetch_concurrency,
            receipt_concurrency,
            rpc_max_attempts,
            retry_base_delay: Duration::from_millis(retry_base_millis),
            max_response_bytes,
            max_blocks_per_run,
            batch_size,
            poll_interval: Duration::from_secs(poll_interval_seconds),
            request_delay: Duration::from_millis(request_delay_millis),
            reorg_max_depth,
            failure_max_attempts,
            repair_max_blocks,
            protocol_registry_refresh_blocks,
            start_block,
            end_block,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_values(values: &[(&str, &str)]) -> Result<Self, ConfigError> {
        let values: HashMap<&str, &str> = values.iter().copied().collect();
        Self::from_lookup(|key| values.get(key).map(|value| (*value).to_string()))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RpcEndpoint(Url);

impl RpcEndpoint {
    pub(crate) fn parse(value: &str) -> Result<Self, ConfigError> {
        let url = Url::parse(value).map_err(|_| ConfigError::InvalidValue {
            key: "BSC_RPC_URL",
            reason: "must be a valid HTTP or HTTPS URL",
        })?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_URL",
                reason: "must be HTTP(S), include a host, contain no user-info, and contain no fragment",
            });
        }
        Ok(Self(url))
    }

    pub(crate) fn as_url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for RpcEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RpcEndpoint([REDACTED])")
    }
}

#[derive(Clone)]
pub struct ClickHouseConfig {
    endpoint: Url,
    user: String,
    password: String,
    database: String,
}

impl ClickHouseConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let local = load_env_file(Path::new(".env"))?;
        Self::from_lookup(|key| {
            env::var(key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| local.get(key).cloned().filter(|value| !value.is_empty()))
        })
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let endpoint_value =
            lookup("BSC_CLICKHOUSE_URL").ok_or(ConfigError::Missing("BSC_CLICKHOUSE_URL"))?;
        let endpoint = Url::parse(&endpoint_value).map_err(|_| ConfigError::InvalidValue {
            key: "BSC_CLICKHOUSE_URL",
            reason: "must be a valid HTTP or HTTPS URL",
        })?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !matches!(endpoint.path(), "" | "/")
        {
            return Err(ConfigError::InvalidValue {
                key: "BSC_CLICKHOUSE_URL",
                reason: "must be an HTTP(S) origin without credentials, path, query, or fragment",
            });
        }

        let user = lookup("BSC_CLICKHOUSE_USER").unwrap_or_else(|| "bsc_admin".to_string());
        validate_safe_identifier("BSC_CLICKHOUSE_USER", &user, 64)?;

        let password = lookup("BSC_CLICKHOUSE_PASSWORD")
            .ok_or(ConfigError::Missing("BSC_CLICKHOUSE_PASSWORD"))?;
        if password.len() > 1024 || password.chars().any(char::is_control) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_CLICKHOUSE_PASSWORD",
                reason: "must be at most 1024 characters and contain no control characters",
            });
        }

        let database = lookup("BSC_CLICKHOUSE_DATABASE")
            .unwrap_or_else(|| BSC_CLICKHOUSE_DATABASE.to_string());
        if database != BSC_CLICKHOUSE_DATABASE {
            return Err(ConfigError::InvalidValue {
                key: "BSC_CLICKHOUSE_DATABASE",
                reason: "must be bsc_aml to preserve network isolation",
            });
        }

        Ok(Self {
            endpoint,
            user,
            password,
            database,
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }

    pub fn database(&self) -> &str {
        &self.database
    }

    #[cfg(test)]
    fn from_values(values: &[(&str, &str)]) -> Result<Self, ConfigError> {
        let values: HashMap<&str, &str> = values.iter().copied().collect();
        Self::from_lookup(|key| values.get(key).map(|value| (*value).to_string()))
    }
}

impl fmt::Debug for ClickHouseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClickHouseConfig")
            .field("endpoint", &"[REDACTED]")
            .field("user", &self.user)
            .field("password", &"[REDACTED]")
            .field("database", &self.database)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: DeploymentMode,
    pub rpc_endpoint: RpcEndpoint,
    pub fallback_rpc_endpoints: Vec<RpcEndpoint>,
    pub rpc_provider: String,
    pub rpc_timeout: Duration,
    pub trace_mode: TraceMode,
    pub require_block_receipts: bool,
    pub trace_probe_block: u64,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let local = load_env_file(Path::new(".env"))?;
        Self::from_lookup(|key| {
            env::var(key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| local.get(key).cloned().filter(|value| !value.is_empty()))
        })
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mode = DeploymentMode::parse(lookup("BSC_MODE").as_deref().unwrap_or("development"))?;
        let rpc_endpoint = lookup("BSC_RPC_URL")
            .ok_or(ConfigError::Missing("BSC_RPC_URL"))
            .and_then(|value| RpcEndpoint::parse(&value))?;
        let fallback_rpc_endpoints =
            parse_fallback_endpoints(lookup("BSC_RPC_FALLBACK_URLS").as_deref(), &rpc_endpoint)?;
        let rpc_provider = lookup("BSC_RPC_PROVIDER").unwrap_or_else(|| "custom".to_string());
        validate_provider_name(&rpc_provider)?;
        let timeout_seconds = parse_number(
            "BSC_RPC_TIMEOUT_SECONDS",
            lookup("BSC_RPC_TIMEOUT_SECONDS"),
            15_u64,
        )?;
        if !(1..=120).contains(&timeout_seconds) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_TIMEOUT_SECONDS",
                reason: "must be between 1 and 120",
            });
        }
        let default_trace_mode = match mode {
            DeploymentMode::Development => "auto",
            DeploymentMode::Production => "required",
        };
        let trace_mode = TraceMode::parse(
            lookup("BSC_TRACE_MODE")
                .as_deref()
                .unwrap_or(default_trace_mode),
        )?;
        let require_block_receipts = parse_bool(
            "BSC_REQUIRE_BLOCK_RECEIPTS",
            lookup("BSC_REQUIRE_BLOCK_RECEIPTS"),
            matches!(mode, DeploymentMode::Production),
        )?;
        let trace_probe_block = parse_number(
            "BSC_TRACE_PROBE_BLOCK",
            lookup("BSC_TRACE_PROBE_BLOCK"),
            0_u64,
        )?;

        if matches!(mode, DeploymentMode::Production) && !trace_mode.required() {
            return Err(ConfigError::InvalidValue {
                key: "BSC_TRACE_MODE",
                reason: "must be required in production",
            });
        }
        if matches!(mode, DeploymentMode::Production) && !require_block_receipts {
            return Err(ConfigError::InvalidValue {
                key: "BSC_REQUIRE_BLOCK_RECEIPTS",
                reason: "must be true in production",
            });
        }

        Ok(Self {
            mode,
            rpc_endpoint,
            fallback_rpc_endpoints,
            rpc_provider,
            rpc_timeout: Duration::from_secs(timeout_seconds),
            trace_mode,
            require_block_receipts,
            trace_probe_block,
        })
    }

    #[cfg(test)]
    fn from_values(values: &[(&str, &str)]) -> Result<Self, ConfigError> {
        let values: HashMap<&str, &str> = values.iter().copied().collect();
        Self::from_lookup(|key| values.get(key).map(|value| (*value).to_string()))
    }
}

fn parse_fallback_endpoints(
    value: Option<&str>,
    primary: &RpcEndpoint,
) -> Result<Vec<RpcEndpoint>, ConfigError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let mut endpoints = Vec::new();
    for raw in value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let endpoint = RpcEndpoint::parse(raw)?;
        if &endpoint == primary || endpoints.contains(&endpoint) {
            return Err(ConfigError::InvalidValue {
                key: "BSC_RPC_FALLBACK_URLS",
                reason: "must not contain duplicate endpoints",
            });
        }
        endpoints.push(endpoint);
    }
    if endpoints.len() > 3 {
        return Err(ConfigError::InvalidValue {
            key: "BSC_RPC_FALLBACK_URLS",
            reason: "supports at most three fallback endpoints",
        });
    }
    Ok(endpoints)
}

fn validate_provider_name(value: &str) -> Result<(), ConfigError> {
    validate_safe_identifier("BSC_RPC_PROVIDER", value, 64)
}

fn validate_safe_identifier(
    key: &'static str,
    value: &str,
    max_length: usize,
) -> Result<(), ConfigError> {
    let valid = !value.is_empty()
        && value.len() <= max_length
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    valid.then_some(()).ok_or(ConfigError::InvalidValue {
        key,
        reason: "contains unsafe or unsupported characters",
    })
}

fn parse_number<T>(key: &'static str, value: Option<String>, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
{
    match value {
        Some(value) => value.parse().map_err(|_| ConfigError::InvalidValue {
            key,
            reason: "must be a valid integer",
        }),
        None => Ok(default),
    }
}

fn parse_optional_number<T>(
    key: &'static str,
    value: Option<String>,
) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
{
    value
        .map(|value| {
            value.parse().map_err(|_| ConfigError::InvalidValue {
                key,
                reason: "must be a valid integer",
            })
        })
        .transpose()
}

fn parse_bool(
    key: &'static str,
    value: Option<String>,
    default: bool,
) -> Result<bool, ConfigError> {
    match value.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
        None => Ok(default),
        Some(value) if matches!(value.as_str(), "true" | "1" | "yes" | "on") => Ok(true),
        Some(value) if matches!(value.as_str(), "false" | "0" | "no" | "off") => Ok(false),
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            reason: "must be true or false",
        }),
    }
}

fn load_env_file(path: &Path) -> Result<HashMap<String, String>, ConfigError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let contents = fs::read_to_string(path).map_err(ConfigError::ReadEnvFile)?;
    parse_env_file(&contents)
}

fn parse_env_file(contents: &str) -> Result<HashMap<String, String>, ConfigError> {
    let mut values = HashMap::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let (key, raw_value) = line
            .split_once('=')
            .ok_or(ConfigError::InvalidEnvLine(index + 1))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            return Err(ConfigError::InvalidEnvLine(index + 1));
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
        values.insert(key.to_string(), value);
    }
    Ok(values)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required setting {0}")]
    Missing(&'static str),
    #[error("invalid setting {key}: {reason}")]
    InvalidValue {
        key: &'static str,
        reason: &'static str,
    },
    #[error("failed to read local .env")]
    ReadEnvFile(#[source] std::io::Error),
    #[error("invalid .env syntax at line {0}")]
    InvalidEnvLine(usize),
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, ClickHouseConfig, ConfigError, DeploymentMode, IngestionConfig,
        ReceiptFetchMode, SyncMode, TraceMode, parse_env_file,
    };

    const URL: (&str, &str) = ("BSC_RPC_URL", "https://rpc.example/secret-path?key=secret");

    #[test]
    fn development_defaults_are_explicitly_degraded_capable() {
        let config = AppConfig::from_values(&[URL]).unwrap();
        assert_eq!(config.mode, DeploymentMode::Development);
        assert_eq!(config.trace_mode, TraceMode::Auto);
        assert!(!config.require_block_receipts);
        assert_eq!(
            format!("{:?}", config.rpc_endpoint),
            "RpcEndpoint([REDACTED])"
        );
        assert!(!format!("{:?}", config).contains("secret"));
    }

    #[test]
    fn production_enforces_trace_and_block_receipts() {
        let config = AppConfig::from_values(&[("BSC_MODE", "production"), URL]).unwrap();
        assert_eq!(config.trace_mode, TraceMode::Required);
        assert!(config.require_block_receipts);

        assert!(matches!(
            AppConfig::from_values(&[
                ("BSC_MODE", "production"),
                ("BSC_TRACE_MODE", "disabled"),
                URL
            ]),
            Err(ConfigError::InvalidValue {
                key: "BSC_TRACE_MODE",
                ..
            })
        ));
        assert!(matches!(
            AppConfig::from_values(&[
                ("BSC_MODE", "production"),
                ("BSC_REQUIRE_BLOCK_RECEIPTS", "false"),
                URL
            ]),
            Err(ConfigError::InvalidValue {
                key: "BSC_REQUIRE_BLOCK_RECEIPTS",
                ..
            })
        ));
    }

    #[test]
    fn rejects_unsafe_or_ambiguous_settings() {
        for url in ["file:///tmp/node", "http://user:pass@node", "not-a-url"] {
            assert!(AppConfig::from_values(&[("BSC_RPC_URL", url)]).is_err());
        }
        assert!(
            AppConfig::from_values(&[
                ("BSC_RPC_URL", "http://node"),
                ("BSC_RPC_PROVIDER", "bad\nlog")
            ])
            .is_err()
        );
        assert!(
            AppConfig::from_values(&[
                ("BSC_RPC_URL", "http://node"),
                ("BSC_RPC_TIMEOUT_SECONDS", "0")
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_strict_local_env_without_expanding_secrets() {
        let parsed = parse_env_file(
            "BSC_RPC_URL='https://node.example/path?token=$TOKEN'\nexport BSC_MODE=development\n",
        )
        .unwrap();
        assert_eq!(
            parsed.get("BSC_RPC_URL").map(String::as_str),
            Some("https://node.example/path?token=$TOKEN")
        );
        assert!(parse_env_file("BROKEN LINE").is_err());
    }

    #[test]
    fn clickhouse_config_is_independent_and_redacts_secrets() {
        let config = ClickHouseConfig::from_values(&[
            ("BSC_CLICKHOUSE_URL", "http://127.0.0.1:38123"),
            ("BSC_CLICKHOUSE_PASSWORD", "database-secret"),
        ])
        .unwrap();

        assert_eq!(config.database(), "bsc_aml");
        assert_eq!(config.user(), "bsc_admin");
        assert!(!format!("{config:?}").contains("database-secret"));
        assert!(!format!("{config:?}").contains("38123"));
    }

    #[test]
    fn clickhouse_config_rejects_cross_chain_or_credentialed_targets() {
        for url in [
            "http://admin:secret@localhost:8123",
            "http://localhost:8123/path",
            "http://localhost:8123?password=secret",
        ] {
            assert!(
                ClickHouseConfig::from_values(&[
                    ("BSC_CLICKHOUSE_URL", url),
                    ("BSC_CLICKHOUSE_PASSWORD", "secret"),
                ])
                .is_err()
            );
        }
        assert!(
            ClickHouseConfig::from_values(&[
                ("BSC_CLICKHOUSE_URL", "http://localhost:8123"),
                ("BSC_CLICKHOUSE_PASSWORD", "secret"),
                ("BSC_CLICKHOUSE_DATABASE", "ethereum_aml"),
            ])
            .is_err()
        );
    }

    #[test]
    fn ingestion_config_is_bounded_and_range_safe() {
        let config = IngestionConfig::from_values(&[
            ("BSC_RECEIPT_FETCH_MODE", "transaction"),
            ("BSC_RECEIPT_CONCURRENCY", "12"),
            ("BSC_INGEST_START_BLOCK", "100"),
            ("BSC_INGEST_END_BLOCK", "105"),
        ])
        .unwrap();
        assert_eq!(config.receipt_fetch_mode, ReceiptFetchMode::Transaction);
        assert_eq!(config.block_fetch_concurrency, 8);
        assert_eq!(config.sync_mode, SyncMode::Auto);
        assert_eq!(config.receipt_concurrency, 12);
        assert_eq!(config.protocol_registry_refresh_blocks, 1_000);
        assert_eq!(config.start_block, Some(100));
        assert_eq!(config.end_block, Some(105));

        assert!(IngestionConfig::from_values(&[("BSC_RECEIPT_CONCURRENCY", "0")]).is_err());
        assert!(
            IngestionConfig::from_values(&[
                ("BSC_INGEST_START_BLOCK", "10"),
                ("BSC_INGEST_END_BLOCK", "9")
            ])
            .is_err()
        );
        assert!(IngestionConfig::from_values(&[("BSC_INGEST_END_BLOCK", "10")]).is_err());
        assert!(
            IngestionConfig::from_values(&[("BSC_PROTOCOL_REGISTRY_REFRESH_BLOCKS", "0")]).is_err()
        );
    }

    #[test]
    fn fallback_endpoints_are_unique_and_bounded() {
        let config = AppConfig::from_values(&[
            ("BSC_RPC_URL", "https://primary.example"),
            (
                "BSC_RPC_FALLBACK_URLS",
                "https://fallback-1.example, https://fallback-2.example",
            ),
        ])
        .unwrap();
        assert_eq!(config.fallback_rpc_endpoints.len(), 2);
        assert!(
            AppConfig::from_values(&[
                ("BSC_RPC_URL", "https://primary.example"),
                ("BSC_RPC_FALLBACK_URLS", "https://primary.example"),
            ])
            .is_err()
        );
    }
}
