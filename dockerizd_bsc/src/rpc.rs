use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{BSC_CHAIN_ID, BSC_NETWORK_ID, config::AppConfig};

const MAX_PROBE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct BscRpcProbe {
    client: Client,
    config: AppConfig,
    next_request_id: std::sync::Arc<AtomicU64>,
}

impl BscRpcProbe {
    pub fn new(config: AppConfig) -> Result<Self, ProbeError> {
        let client = Client::builder()
            .timeout(config.rpc_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("aml-whole-bsc-capability-probe/0.1")
            .build()
            .map_err(ProbeError::BuildClient)?;
        Ok(Self {
            client,
            config,
            next_request_id: std::sync::Arc::new(AtomicU64::new(1)),
        })
    }

    pub async fn inspect(&self) -> Result<ProbeReport, ProbeError> {
        let chain_id: String = self.call("eth_chainId", json!([])).await?;
        let chain_id = parse_quantity("eth_chainId", &chain_id)?;
        if chain_id != BSC_CHAIN_ID {
            return Err(ProbeError::WrongChain {
                actual: chain_id,
                expected: BSC_CHAIN_ID,
            });
        }

        let client_version: String = self.call("web3_clientVersion", json!([])).await?;
        let client_version = sanitize_text(&client_version, 160);
        let latest: String = self.call("eth_blockNumber", json!([])).await?;
        let latest_block = parse_quantity("eth_blockNumber", &latest)?;

        let (finalized, finalized_block) = match self
            .call_value("eth_getBlockByNumber", json!(["finalized", false]))
            .await
        {
            Ok(value) => match block_number(&value) {
                Ok(number) => (
                    Capability::available("finalized block returned"),
                    Some(number),
                ),
                Err(error) => (Capability::unavailable(error.summary()), None),
            },
            Err(error) => (Capability::unavailable(error.summary()), None),
        };

        let receipt_block = finalized_block.unwrap_or(self.config.trace_probe_block);
        let block_receipts = match self
            .call_value(
                "eth_getBlockReceipts",
                json!([format!("0x{receipt_block:x}")]),
            )
            .await
        {
            Ok(Value::Array(_)) => Capability::available("block receipt array returned"),
            Ok(_) => Capability::unavailable("RPC returned a non-array receipt result"),
            Err(error) => Capability::unavailable(error.summary()),
        };

        let internal_traces = if self.config.trace_mode.enabled() {
            match self
                .call_value(
                    "debug_traceBlockByNumber",
                    json!([
                        format!("0x{:x}", self.config.trace_probe_block),
                        {
                            "tracer": "callTracer",
                            "timeout": "5s",
                            "tracerConfig": { "onlyTopCall": true }
                        }
                    ]),
                )
                .await
            {
                Ok(Value::Array(_)) => Capability::available("callTracer block array returned"),
                Ok(_) => Capability::unavailable("RPC returned a non-array trace result"),
                Err(error) => Capability::unavailable(error.summary()),
            }
        } else {
            Capability::unavailable("disabled by configuration")
        };

        let requirements = ProbeRequirements {
            finalized: true,
            block_receipts: self.config.require_block_receipts,
            internal_traces: self.config.trace_mode.required(),
        };
        let mut report = ProbeReport {
            status: ProbeStatus::Ready,
            mode: self.config.mode.to_string(),
            network_id: BSC_NETWORK_ID.to_string(),
            chain_id,
            provider: self.config.rpc_provider.clone(),
            client_version,
            latest_block,
            finalized_block,
            capabilities: ProbeCapabilities {
                finalized,
                block_receipts,
                internal_traces,
            },
            requirements,
            trace_mode: self.config.trace_mode.to_string(),
        };
        report.status = report.calculate_status();
        Ok(report)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: Value,
    ) -> Result<T, ProbeError> {
        let value = self.call_value(method, params).await?;
        serde_json::from_value(value).map_err(|_| {
            ProbeError::Rpc(RpcCallError::InvalidResponse {
                method,
                reason: "result has an unexpected type",
            })
        })
    }

    async fn call_value(&self, method: &'static str, params: Value) -> Result<Value, ProbeError> {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let response = self
            .client
            .post(self.config.rpc_endpoint.as_url().clone())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }))
            .send()
            .await
            .map_err(|source| ProbeError::Rpc(RpcCallError::Transport { method, source }))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ProbeError::Rpc(RpcCallError::HttpStatus { method, status }));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PROBE_RESPONSE_BYTES as u64)
        {
            return Err(ProbeError::Rpc(RpcCallError::ResponseTooLarge { method }));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|source| ProbeError::Rpc(RpcCallError::Transport { method, source }))?;
        if bytes.len() > MAX_PROBE_RESPONSE_BYTES {
            return Err(ProbeError::Rpc(RpcCallError::ResponseTooLarge { method }));
        }
        let response: RpcResponse = serde_json::from_slice(&bytes).map_err(|_| {
            ProbeError::Rpc(RpcCallError::InvalidResponse {
                method,
                reason: "body is not a valid JSON-RPC response",
            })
        })?;
        if response.jsonrpc.as_deref() != Some("2.0") || response.id != json!(id) {
            return Err(ProbeError::Rpc(RpcCallError::InvalidResponse {
                method,
                reason: "jsonrpc version or request id does not match",
            }));
        }
        if let Some(error) = response.error {
            return Err(ProbeError::Rpc(RpcCallError::Server {
                method,
                code: error.code,
            }));
        }
        response.result.ok_or_else(|| {
            ProbeError::Rpc(RpcCallError::InvalidResponse {
                method,
                reason: "response contains neither result nor error",
            })
        })
    }
}

pub async fn probe_configured_endpoints(config: &AppConfig) -> Result<ProbeReport, ProbeError> {
    let primary = BscRpcProbe::new(config.clone())?.inspect().await?;
    primary.ensure_requirements()?;
    for endpoint in &config.fallback_rpc_endpoints {
        let mut fallback_config = config.clone();
        fallback_config.rpc_endpoint = endpoint.clone();
        fallback_config.fallback_rpc_endpoints.clear();
        let fallback = BscRpcProbe::new(fallback_config)?.inspect().await?;
        fallback.ensure_requirements()?;
    }
    Ok(primary)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Capability {
    pub available: bool,
    pub detail: String,
}

impl Capability {
    fn available(detail: &str) -> Self {
        Self {
            available: true,
            detail: detail.to_string(),
        }
    }

    fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            available: false,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProbeCapabilities {
    pub finalized: Capability,
    pub block_receipts: Capability,
    pub internal_traces: Capability,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProbeRequirements {
    pub finalized: bool,
    pub block_receipts: bool,
    pub internal_traces: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Ready,
    Degraded,
    Unready,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProbeReport {
    pub status: ProbeStatus,
    pub mode: String,
    pub network_id: String,
    pub chain_id: u64,
    pub provider: String,
    pub client_version: String,
    pub latest_block: u64,
    pub finalized_block: Option<u64>,
    pub capabilities: ProbeCapabilities,
    pub requirements: ProbeRequirements,
    pub trace_mode: String,
}

impl ProbeReport {
    pub fn missing_required_capabilities(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.requirements.finalized && !self.capabilities.finalized.available {
            missing.push("finalized");
        }
        if self.requirements.block_receipts && !self.capabilities.block_receipts.available {
            missing.push("block_receipts");
        }
        if self.requirements.internal_traces && !self.capabilities.internal_traces.available {
            missing.push("internal_traces");
        }
        missing
    }

    pub fn ensure_requirements(&self) -> Result<(), ProbeError> {
        let missing = self.missing_required_capabilities();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(ProbeError::MissingCapabilities(missing.join(", ")))
        }
    }

    fn calculate_status(&self) -> ProbeStatus {
        if !self.missing_required_capabilities().is_empty() {
            ProbeStatus::Unready
        } else if !self.capabilities.block_receipts.available
            || (self.trace_mode != "disabled" && !self.capabilities.internal_traces.available)
        {
            ProbeStatus::Degraded
        } else {
            ProbeStatus::Ready
        }
    }
}

fn parse_quantity(method: &'static str, value: &str) -> Result<u64, ProbeError> {
    let Some(hex) = value.strip_prefix("0x") else {
        return Err(ProbeError::Rpc(RpcCallError::InvalidResponse {
            method,
            reason: "quantity is not 0x-prefixed",
        }));
    };
    if hex.is_empty() {
        return Err(ProbeError::Rpc(RpcCallError::InvalidResponse {
            method,
            reason: "quantity is empty",
        }));
    }
    u64::from_str_radix(hex, 16).map_err(|_| {
        ProbeError::Rpc(RpcCallError::InvalidResponse {
            method,
            reason: "quantity does not fit UInt64",
        })
    })
}

fn block_number(value: &Value) -> Result<u64, RpcCallError> {
    value
        .get("number")
        .and_then(Value::as_str)
        .ok_or(RpcCallError::InvalidResponse {
            method: "eth_getBlockByNumber",
            reason: "finalized block is null or has no number",
        })
        .and_then(|number| {
            parse_quantity("eth_getBlockByNumber", number).map_err(|error| match error {
                ProbeError::Rpc(error) => error,
                _ => unreachable!("quantity parser returns only RPC errors"),
            })
        })
}

fn sanitize_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
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
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("failed to build the RPC HTTP client")]
    BuildClient(#[source] reqwest::Error),
    #[error("BSC RPC returned chain id {actual}, expected {expected}")]
    WrongChain { actual: u64, expected: u64 },
    #[error("required RPC capabilities are unavailable: {0}")]
    MissingCapabilities(String),
    #[error(transparent)]
    Rpc(#[from] RpcCallError),
}

impl ProbeError {
    fn summary(&self) -> String {
        match self {
            Self::Rpc(error) => error.summary(),
            Self::BuildClient(_) => "HTTP client initialization failed".to_string(),
            Self::WrongChain { .. } => "endpoint belongs to another chain".to_string(),
            Self::MissingCapabilities(_) => "required capabilities are unavailable".to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RpcCallError {
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
    #[error("RPC error {code} for {method}")]
    Server { method: &'static str, code: i64 },
    #[error("invalid RPC response for {method}: {reason}")]
    InvalidResponse {
        method: &'static str,
        reason: &'static str,
    },
    #[error("RPC response is too large for capability probe {method}")]
    ResponseTooLarge { method: &'static str },
}

impl RpcCallError {
    fn summary(&self) -> String {
        match self {
            Self::Server { code, .. } => format!("JSON-RPC error {code}"),
            Self::HttpStatus { status, .. } => format!("HTTP {status}"),
            Self::Transport { .. } => "transport failure".to_string(),
            Self::InvalidResponse { reason, .. } => (*reason).to_string(),
            Self::ResponseTooLarge { .. } => "response exceeded probe limit".to_string(),
        }
    }
}

impl fmt::Display for ProbeStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Unready => "unready",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::{Value, json};

    use super::{BscRpcProbe, ProbeError, ProbeStatus};
    use crate::config::{AppConfig, DeploymentMode, RpcEndpoint, TraceMode};

    #[derive(Clone)]
    struct MockRpc {
        chain_id: &'static str,
        finalized: bool,
        receipts: bool,
        traces: bool,
    }

    async fn rpc(State(state): State<Arc<MockRpc>>, Json(request): Json<Value>) -> Json<Value> {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "eth_chainId" => Some(json!(state.chain_id)),
            "web3_clientVersion" => Some(json!("bsc/mock\nsecret-free")),
            "eth_blockNumber" => Some(json!("0x65")),
            "eth_getBlockByNumber" if state.finalized => Some(json!({ "number": "0x64" })),
            "eth_getBlockReceipts" if state.receipts => Some(json!([])),
            "debug_traceBlockByNumber" if state.traces => Some(json!([])),
            _ => None,
        };
        Json(match result {
            Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            None => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "not found" } })
            }
        })
    }

    async fn start_mock(state: MockRpc) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/", post(rpc))
            .with_state(Arc::new(state));
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), handle)
    }

    fn config(url: &str, mode: DeploymentMode, trace_mode: TraceMode) -> AppConfig {
        AppConfig {
            mode,
            rpc_endpoint: RpcEndpoint::parse(url).unwrap(),
            fallback_rpc_endpoints: Vec::new(),
            rpc_provider: "test".to_string(),
            rpc_timeout: std::time::Duration::from_secs(3),
            trace_mode,
            require_block_receipts: matches!(mode, DeploymentMode::Production),
            trace_probe_block: 0,
        }
    }

    #[tokio::test]
    async fn production_accepts_complete_bsc_endpoint() {
        let (url, server) = start_mock(MockRpc {
            chain_id: "0x38",
            finalized: true,
            receipts: true,
            traces: true,
        })
        .await;
        let report = BscRpcProbe::new(config(
            &url,
            DeploymentMode::Production,
            TraceMode::Required,
        ))
        .unwrap()
        .inspect()
        .await
        .unwrap();
        server.abort();

        assert_eq!(report.status, ProbeStatus::Ready);
        assert_eq!(report.chain_id, 56);
        assert_eq!(report.finalized_block, Some(100));
        assert!(
            report
                .client_version
                .chars()
                .all(|character| !character.is_control())
        );
        report.ensure_requirements().unwrap();
    }

    #[tokio::test]
    async fn rejects_ethereum_endpoint_before_capability_checks() {
        let (url, server) = start_mock(MockRpc {
            chain_id: "0x1",
            finalized: true,
            receipts: true,
            traces: true,
        })
        .await;
        let result = BscRpcProbe::new(config(
            &url,
            DeploymentMode::Production,
            TraceMode::Required,
        ))
        .unwrap()
        .inspect()
        .await;
        server.abort();

        assert!(matches!(
            result,
            Err(ProbeError::WrongChain {
                actual: 1,
                expected: 56
            })
        ));
    }

    #[tokio::test]
    async fn production_refuses_missing_required_capabilities() {
        let (url, server) = start_mock(MockRpc {
            chain_id: "0x38",
            finalized: false,
            receipts: false,
            traces: false,
        })
        .await;
        let report = BscRpcProbe::new(config(
            &url,
            DeploymentMode::Production,
            TraceMode::Required,
        ))
        .unwrap()
        .inspect()
        .await
        .unwrap();
        server.abort();

        assert_eq!(report.status, ProbeStatus::Unready);
        assert_eq!(
            report.missing_required_capabilities(),
            vec!["finalized", "block_receipts", "internal_traces"]
        );
        assert!(matches!(
            report.ensure_requirements(),
            Err(ProbeError::MissingCapabilities(_))
        ));
    }

    #[tokio::test]
    async fn development_reports_optional_trace_gap_as_degraded() {
        let (url, server) = start_mock(MockRpc {
            chain_id: "0x38",
            finalized: true,
            receipts: true,
            traces: false,
        })
        .await;
        let report = BscRpcProbe::new(config(&url, DeploymentMode::Development, TraceMode::Auto))
            .unwrap()
            .inspect()
            .await
            .unwrap();
        server.abort();

        assert_eq!(report.status, ProbeStatus::Degraded);
        assert!(report.missing_required_capabilities().is_empty());
        report.ensure_requirements().unwrap();
    }
}
