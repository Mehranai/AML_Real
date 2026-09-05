use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{task::JoinSet, time::sleep};

use crate::config::{AppConfig, IngestionConfig, ReceiptFetchMode, TraceMode};

use super::model::{RpcBlock, RpcBlockTrace, RpcReceipt};

pub(crate) struct FetchedBlock {
    pub block: RpcBlock,
    pub receipts: Vec<RpcReceipt>,
    pub traces: Option<Vec<RpcBlockTrace>>,
}

#[derive(Clone)]
pub(crate) struct CanonicalRpcClient {
    client: Client,
    endpoints: Arc<Vec<reqwest::Url>>,
    settings: IngestionConfig,
    trace_mode: TraceMode,
    next_request_id: Arc<AtomicU64>,
    next_endpoint: Arc<AtomicU64>,
}

impl CanonicalRpcClient {
    pub(crate) fn new(
        app_config: &AppConfig,
        settings: IngestionConfig,
    ) -> Result<Self, RpcFetchError> {
        let client = Client::builder()
            .timeout(app_config.rpc_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("aml-whole-bsc-canonical-ingestion/0.1")
            .build()
            .map_err(RpcFetchError::BuildClient)?;
        let endpoints = std::iter::once(app_config.rpc_endpoint.as_url().clone())
            .chain(
                app_config
                    .fallback_rpc_endpoints
                    .iter()
                    .map(|endpoint| endpoint.as_url().clone()),
            )
            .collect::<Vec<_>>();
        if settings.rpc_max_attempts < u32::try_from(endpoints.len()).unwrap_or(u32::MAX) {
            return Err(RpcFetchError::InsufficientFailoverAttempts {
                endpoints: endpoints.len(),
                attempts: settings.rpc_max_attempts,
            });
        }
        Ok(Self {
            client,
            endpoints: Arc::new(endpoints),
            settings,
            trace_mode: app_config.trace_mode,
            next_request_id: Arc::new(AtomicU64::new(1)),
            next_endpoint: Arc::new(AtomicU64::new(0)),
        })
    }

    pub(crate) async fn finalized_height(&self) -> Result<u64, RpcFetchError> {
        let value = self
            .call_value("eth_getBlockByNumber", json!(["finalized", false]))
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::FinalizedBlockUnavailable);
        }
        let number =
            value
                .get("number")
                .and_then(Value::as_str)
                .ok_or(RpcFetchError::InvalidResponse {
                    method: "eth_getBlockByNumber",
                    reason: "finalized block has no number",
                })?;
        parse_u64_quantity("eth_getBlockByNumber", number)
    }

    pub(crate) async fn block_identity(
        &self,
        block_number: u64,
    ) -> Result<RemoteBlockIdentity, RpcFetchError> {
        let value = self
            .call_value(
                "eth_getBlockByNumber",
                json!([format!("0x{block_number:x}"), false]),
            )
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::BlockUnavailable { block_number });
        }
        decode_block_identity("eth_getBlockByNumber", value)
    }

    pub(crate) async fn block_identity_by_hash(
        &self,
        block_hash: &str,
    ) -> Result<RemoteBlockIdentity, RpcFetchError> {
        let block_hash = normalize_rpc_hash("eth_getBlockByHash", block_hash)?;
        let value = self
            .call_value("eth_getBlockByHash", json!([block_hash, false]))
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::BlockHashUnavailable);
        }
        decode_block_identity("eth_getBlockByHash", value)
    }

    pub(crate) async fn block_with_receipts(
        &self,
        block_number: u64,
    ) -> Result<FetchedBlock, RpcFetchError> {
        let block_tag = format!("0x{block_number:x}");
        let value = self
            .call_value("eth_getBlockByNumber", json!([block_tag, true]))
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::BlockUnavailable { block_number });
        }
        let block: RpcBlock = decode("eth_getBlockByNumber", value)?;
        let receipts = match self.settings.receipt_fetch_mode {
            ReceiptFetchMode::Block => self.fetch_block_receipts(block_number).await?,
            ReceiptFetchMode::Transaction => {
                self.fetch_transaction_receipts(&block.transactions).await?
            }
            ReceiptFetchMode::Auto => match self.fetch_block_receipts(block_number).await {
                Ok(receipts) => receipts,
                Err(error) if error.is_method_unavailable() => {
                    self.fetch_transaction_receipts(&block.transactions).await?
                }
                Err(error) => return Err(error),
            },
        };
        let traces = match self.trace_mode {
            TraceMode::Disabled => None,
            TraceMode::Auto => match self.fetch_block_traces(block_number).await {
                Ok(traces) => Some(traces),
                Err(error) if error.is_method_unavailable() => None,
                Err(error) => return Err(error),
            },
            TraceMode::Required => Some(self.fetch_block_traces(block_number).await?),
        };
        Ok(FetchedBlock {
            block,
            receipts,
            traces,
        })
    }

    async fn fetch_block_traces(
        &self,
        block_number: u64,
    ) -> Result<Vec<RpcBlockTrace>, RpcFetchError> {
        let value = self
            .call_value(
                "debug_traceBlockByNumber",
                json!([
                    format!("0x{block_number:x}"),
                    {
                        "tracer": "callTracer",
                        "timeout": "30s",
                        "tracerConfig": { "onlyTopCall": false, "withLog": false }
                    }
                ]),
            )
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::TraceDataUnavailable);
        }
        decode("debug_traceBlockByNumber", value)
    }

    async fn fetch_block_receipts(
        &self,
        block_number: u64,
    ) -> Result<Vec<RpcReceipt>, RpcFetchError> {
        let value = self
            .call_value(
                "eth_getBlockReceipts",
                json!([format!("0x{block_number:x}")]),
            )
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::ReceiptDataUnavailable {
                method: "eth_getBlockReceipts",
            });
        }
        decode("eth_getBlockReceipts", value)
    }

    async fn fetch_transaction_receipts(
        &self,
        transactions: &[super::model::RpcTransaction],
    ) -> Result<Vec<RpcReceipt>, RpcFetchError> {
        let mut receipts = Vec::with_capacity(transactions.len());
        for chunk in transactions.chunks(self.settings.receipt_concurrency) {
            let mut tasks = JoinSet::new();
            for transaction in chunk {
                let client = self.clone();
                let tx_hash = transaction.hash.clone();
                tasks.spawn(async move { client.fetch_transaction_receipt(tx_hash).await });
            }
            while let Some(result) = tasks.join_next().await {
                receipts.push(result.map_err(RpcFetchError::ReceiptTask)??);
            }
        }
        Ok(receipts)
    }

    async fn fetch_transaction_receipt(
        &self,
        tx_hash: String,
    ) -> Result<RpcReceipt, RpcFetchError> {
        let value = self
            .call_value("eth_getTransactionReceipt", json!([tx_hash]))
            .await?;
        if value.is_null() {
            return Err(RpcFetchError::ReceiptDataUnavailable {
                method: "eth_getTransactionReceipt",
            });
        }
        decode("eth_getTransactionReceipt", value)
    }

    async fn call_value(
        &self,
        method: &'static str,
        params: Value,
    ) -> Result<Value, RpcFetchError> {
        let endpoint_count = self.endpoints.len();
        let start_index = usize::try_from(self.next_endpoint.fetch_add(1, Ordering::Relaxed))
            .unwrap_or(0)
            % endpoint_count;
        for attempt in 1..=self.settings.rpc_max_attempts {
            let endpoint_index =
                (start_index + usize::try_from(attempt - 1).unwrap_or(0)) % endpoint_count;
            match self
                .call_once(&self.endpoints[endpoint_index], method, params.clone())
                .await
            {
                Ok(value) => return Ok(value),
                Err(error)
                    if error.failover_worthy() && attempt < self.settings.rpc_max_attempts =>
                {
                    if error.retryable() {
                        sleep(self.retry_delay(attempt)).await;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("RPC attempt loop always returns")
    }

    async fn call_once(
        &self,
        endpoint: &reqwest::Url,
        method: &'static str,
        params: Value,
    ) -> Result<Value, RpcFetchError> {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let mut response = self
            .client
            .post(endpoint.clone())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }))
            .send()
            .await
            .map_err(|source| RpcFetchError::Transport { method, source })?;
        let status = response.status();
        if !status.is_success() {
            return Err(RpcFetchError::HttpStatus { method, status });
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.settings.max_response_bytes as u64)
        {
            return Err(RpcFetchError::ResponseTooLarge { method });
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|source| RpcFetchError::Transport { method, source })?
        {
            if bytes.len().saturating_add(chunk.len()) > self.settings.max_response_bytes {
                return Err(RpcFetchError::ResponseTooLarge { method });
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: RpcResponse =
            serde_json::from_slice(&bytes).map_err(|_| RpcFetchError::InvalidResponse {
                method,
                reason: "body is not a valid JSON-RPC response",
            })?;
        if response.jsonrpc.as_deref() != Some("2.0") || response.id != json!(id) {
            return Err(RpcFetchError::InvalidResponse {
                method,
                reason: "jsonrpc version or request id does not match",
            });
        }
        if let Some(error) = response.error {
            let method_unavailable = server_method_unavailable(error.code, &error.message);
            return Err(RpcFetchError::Server {
                method,
                code: error.code,
                method_unavailable,
            });
        }
        response.result.ok_or(RpcFetchError::InvalidResponse {
            method,
            reason: "response contains neither result nor error",
        })
    }

    fn retry_delay(&self, attempt: u32) -> Duration {
        let multiplier = 1_u32 << attempt.saturating_sub(1).min(6);
        let base = self
            .settings
            .retry_base_delay
            .checked_mul(multiplier)
            .unwrap_or(Duration::from_secs(10))
            .min(Duration::from_secs(10));
        let base_millis = u64::try_from(base.as_millis()).unwrap_or(10_000);
        let jitter_ceiling = (base_millis / 4).max(1);
        let jitter = self.next_request_id.load(Ordering::Relaxed) % jitter_ceiling;
        base.saturating_add(Duration::from_millis(jitter))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteBlockIdentity {
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcBlockIdentity {
    number: Option<String>,
    hash: Option<String>,
    parent_hash: String,
}

fn decode_block_identity(
    method: &'static str,
    value: Value,
) -> Result<RemoteBlockIdentity, RpcFetchError> {
    let identity: RpcBlockIdentity = decode(method, value)?;
    Ok(RemoteBlockIdentity {
        number: parse_u64_quantity(
            method,
            identity
                .number
                .as_deref()
                .ok_or(RpcFetchError::InvalidResponse {
                    method,
                    reason: "block identity has no number",
                })?,
        )?,
        hash: normalize_rpc_hash(
            method,
            identity
                .hash
                .as_deref()
                .ok_or(RpcFetchError::InvalidResponse {
                    method,
                    reason: "block identity has no hash",
                })?,
        )?,
        parent_hash: normalize_rpc_hash(method, &identity.parent_hash)?,
    })
}

fn normalize_rpc_hash(method: &'static str, value: &str) -> Result<String, RpcFetchError> {
    if value.len() != 66
        || !value.starts_with("0x")
        || !value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RpcFetchError::InvalidResponse {
            method,
            reason: "block hash is not 32-byte 0x-prefixed hex",
        });
    }
    Ok(value.to_ascii_lowercase())
}

fn server_method_unavailable(code: i64, message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    code == -32601
        || [
            "method not found",
            "does not exist",
            "not available",
            "not enabled",
            "unsupported",
            "unknown method",
            "missing trie node",
            "historical state unavailable",
            "state is not available",
        ]
        .iter()
        .any(|needle| message.contains(needle))
}

fn decode<T: DeserializeOwned>(method: &'static str, value: Value) -> Result<T, RpcFetchError> {
    serde_json::from_value(value).map_err(|_| RpcFetchError::InvalidResponse {
        method,
        reason: "result has an unexpected type or missing required fields",
    })
}

fn parse_u64_quantity(method: &'static str, value: &str) -> Result<u64, RpcFetchError> {
    let Some(hex) = value.strip_prefix("0x") else {
        return Err(RpcFetchError::InvalidResponse {
            method,
            reason: "quantity is not 0x-prefixed",
        });
    };
    if hex.is_empty() || (hex.len() > 1 && hex.starts_with('0')) {
        return Err(RpcFetchError::InvalidResponse {
            method,
            reason: "quantity is empty or has leading zeroes",
        });
    }
    u64::from_str_radix(hex, 16).map_err(|_| RpcFetchError::InvalidResponse {
        method,
        reason: "quantity does not fit UInt64",
    })
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    jsonrpc: Option<String>,
    id: Value,
    result: Option<Value>,
    error: Option<RpcErrorPayload>,
}

#[derive(Debug, Deserialize)]
struct RpcErrorPayload {
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcFetchError {
    #[error("failed to build the canonical ingestion HTTP client")]
    BuildClient(#[source] reqwest::Error),
    #[error("configured {endpoints} RPC endpoints but only {attempts} attempts per request")]
    InsufficientFailoverAttempts { endpoints: usize, attempts: u32 },
    #[error("RPC transport failed for {method}")]
    Transport {
        method: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("RPC returned HTTP {status} for {method}")]
    HttpStatus {
        method: &'static str,
        status: StatusCode,
    },
    #[error("RPC returned JSON-RPC error {code} for {method}")]
    Server {
        method: &'static str,
        code: i64,
        method_unavailable: bool,
    },
    #[error("invalid RPC response for {method}: {reason}")]
    InvalidResponse {
        method: &'static str,
        reason: &'static str,
    },
    #[error("RPC response exceeded the configured size limit for {method}")]
    ResponseTooLarge { method: &'static str },
    #[error("finalized block is unavailable")]
    FinalizedBlockUnavailable,
    #[error("block {block_number} is unavailable")]
    BlockUnavailable { block_number: u64 },
    #[error("block hash is unavailable")]
    BlockHashUnavailable,
    #[error("receipt data is unavailable from {method}")]
    ReceiptDataUnavailable { method: &'static str },
    #[error("callTracer data is unavailable")]
    TraceDataUnavailable,
    #[error("receipt worker failed")]
    ReceiptTask(#[source] tokio::task::JoinError),
}

impl RpcFetchError {
    pub(crate) fn retryable(&self) -> bool {
        match self {
            Self::Transport { source, .. } => {
                source.is_timeout()
                    || source.is_connect()
                    || source.is_request()
                    || source.is_body()
            }
            Self::HttpStatus { status, .. } => {
                *status == StatusCode::REQUEST_TIMEOUT
                    || *status == StatusCode::TOO_MANY_REQUESTS
                    || status.is_server_error()
            }
            Self::Server {
                code,
                method_unavailable,
                ..
            } => !method_unavailable && matches!(*code, -32000 | -32005 | -32603),
            _ => false,
        }
    }

    fn is_method_unavailable(&self) -> bool {
        matches!(
            self,
            Self::Server {
                method_unavailable: true,
                ..
            }
        )
    }

    fn failover_worthy(&self) -> bool {
        self.retryable() || self.is_method_unavailable()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::{Value, json};

    use super::{CanonicalRpcClient, RpcFetchError, parse_u64_quantity, server_method_unavailable};
    use crate::config::{AppConfig, DeploymentMode, IngestionConfig, RpcEndpoint, TraceMode};

    #[test]
    fn quantity_parser_is_strict() {
        assert_eq!(parse_u64_quantity("test", "0x38").unwrap(), 56);
        assert!(parse_u64_quantity("test", "38").is_err());
        assert!(parse_u64_quantity("test", "0x00").is_err());
    }

    #[test]
    fn distinguishes_trace_coverage_gaps_from_retryable_server_failures() {
        assert!(server_method_unavailable(-32601, "method not found"));
        assert!(server_method_unavailable(-32000, "missing trie node"));
        assert!(!server_method_unavailable(
            -32000,
            "temporary backend failure"
        ));
        assert!(!server_method_unavailable(-32005, "rate limit exceeded"));
    }

    async fn finalized_rpc(
        State(available): State<Arc<bool>>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if *available {
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "number": "0x64" }
            }))
        } else {
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32005, "message": "rate limited" }
            }))
        }
    }

    async fn start_rpc(available: bool) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/", post(finalized_rpc))
            .with_state(Arc::new(available));
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), handle)
    }

    #[tokio::test]
    async fn retries_on_the_next_validated_endpoint() {
        let (primary, primary_server) = start_rpc(false).await;
        let (fallback, fallback_server) = start_rpc(true).await;
        let app = AppConfig {
            mode: DeploymentMode::Development,
            rpc_endpoint: RpcEndpoint::parse(&primary).unwrap(),
            fallback_rpc_endpoints: vec![RpcEndpoint::parse(&fallback).unwrap()],
            rpc_provider: "test".to_string(),
            rpc_timeout: std::time::Duration::from_secs(2),
            trace_mode: TraceMode::Disabled,
            require_block_receipts: false,
            trace_probe_block: 0,
        };
        let settings = IngestionConfig::from_values(&[
            ("BSC_RPC_MAX_ATTEMPTS", "2"),
            ("BSC_RPC_RETRY_BASE_MILLIS", "10"),
        ])
        .unwrap();
        let client = CanonicalRpcClient::new(&app, settings).unwrap();
        assert_eq!(client.finalized_height().await.unwrap(), 100);

        let insufficient = IngestionConfig::from_values(&[("BSC_RPC_MAX_ATTEMPTS", "1")]).unwrap();
        assert!(matches!(
            CanonicalRpcClient::new(&app, insufficient),
            Err(RpcFetchError::InsufficientFailoverAttempts { .. })
        ));
        primary_server.abort();
        fallback_server.abort();
    }
}
