use crate::{
    config::{self, RpcPolicy},
    model,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

#[derive(Debug)]
struct Failure {
    retryable: bool,
    reason: &'static str,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bitcoin RPC {}", self.reason)
    }
}
impl std::error::Error for Failure {}
fn failure(reason: &'static str, retryable: bool) -> Failure {
    Failure { reason, retryable }
}

#[derive(Clone)]
pub struct Rpc {
    client: reqwest::Client,
    url: reqwest::Url,
    user: String,
    password: String,
    sequence: Arc<AtomicU64>,
    next_request: Arc<Mutex<Instant>>,
    policy: RpcPolicy,
}
impl Rpc {
    pub fn from_env() -> Result<Self> {
        Self::probe_from_env()?.with_policy(RpcPolicy::from_env()?)
    }
    /// A probe inspects only its configured primary, without hidden failover/retry.
    pub fn probe_from_env() -> Result<Self> {
        Self::new(
            &config::required("BITCOIN_RPC_URL")?,
            config::required("BITCOIN_RPC_USER")?,
            config::required("BITCOIN_RPC_PASSWORD")?,
            config::bounded("BITCOIN_RPC_TIMEOUT_SECONDS", 60, 1, 600)?,
        )
    }
    pub fn new(url: &str, user: String, password: String, timeout: u64) -> Result<Self> {
        Ok(Self {
            client: config::http(timeout)?,
            url: config::origin(url)?,
            user,
            password,
            sequence: Arc::new(AtomicU64::new(1)),
            next_request: Arc::new(Mutex::new(Instant::now())),
            policy: RpcPolicy::default(),
        })
    }
    pub fn with_policy(mut self, policy: RpcPolicy) -> Result<Self> {
        ensure!(
            (1..=8).contains(&policy.attempts)
                && policy.retry_base_ms <= 5000
                && policy.interval_ms <= 10000
                && (1024..=128 * 1024 * 1024).contains(&policy.max_response_bytes),
            "invalid Bitcoin RPC policy"
        );
        self.policy = policy;
        Ok(self)
    }
    async fn send(&self, payload: &Value) -> std::result::Result<Value, Failure> {
        {
            let mut next = self.next_request.lock().await;
            tokio::time::sleep_until(*next).await;
            *next = Instant::now() + Duration::from_millis(self.policy.interval_ms);
        }
        let mut response = self
            .client
            .post(self.url.clone())
            .basic_auth(&self.user, Some(&self.password))
            .json(payload)
            .send()
            .await
            .map_err(|_| failure("transport failed", true))?;
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403) {
            return Err(failure("authentication/authorization failed", false));
        }
        if status.as_u16() == 429 {
            return Err(failure("rate limited", true));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure("response interrupted", true))?
        {
            if bytes.len().saturating_add(chunk.len()) > self.policy.max_response_bytes {
                return Err(failure("response exceeds configured byte limit", false));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| failure("invalid JSON", status.is_server_error()))?;
        if !status.is_success() && !status.is_server_error() {
            return Err(failure("HTTP request rejected", false));
        }
        if status.is_server_error()
            && value.as_array().map_or(value["error"].is_null(), |rows| {
                rows.iter().any(|row| row["error"].is_null())
            })
        {
            return Err(failure("server failed", true));
        }
        Ok(value)
    }
    async fn dispatch(&self, requests: Vec<Value>, batch: bool) -> Result<Vec<Value>> {
        let ids: Vec<u64> = requests.iter().map(|r| r["id"].as_u64().unwrap()).collect();
        let payload = if batch {
            Value::Array(requests)
        } else {
            requests.into_iter().next().unwrap()
        };
        for attempt in 0..self.policy.attempts {
            let result = self
                .send(&payload)
                .await
                .and_then(|value| decode(value, &ids, batch));
            match result {
                Ok(values) => return Ok(values),
                Err(error) if error.retryable && attempt + 1 < self.policy.attempts => {
                    let delay = self
                        .policy
                        .retry_base_ms
                        .saturating_mul(1u64 << attempt)
                        .min(10000);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("policy requires at least one attempt")
    }
    fn request(&self, method: &str, params: Value) -> Result<Value> {
        ensure!(
            matches!(
                method,
                "getblockchaininfo"
                    | "getblockcount"
                    | "getblockhash"
                    | "getblock"
                    | "getindexinfo"
                    | "getrawtransaction"
            ),
            "Bitcoin RPC method is not read-only/allowlisted"
        );
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        Ok(json!({"jsonrpc":"1.0","id":id,"method":method,"params":params}))
    }
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        Ok(self
            .dispatch(vec![self.request(method, params)?], false)
            .await?
            .remove(0))
    }
    /// Responses may arrive in any order; every expected id must occur exactly once.
    pub async fn batch(&self, calls: Vec<(&str, Value)>) -> Result<Vec<Value>> {
        ensure!(
            !calls.is_empty() && calls.len() <= 256,
            "Bitcoin RPC batch must contain 1..256 calls"
        );
        if !self.policy.batch_enabled || calls.len() == 1 {
            let mut values = Vec::with_capacity(calls.len());
            for (method, params) in calls {
                values.push(self.call(method, params).await?);
            }
            return Ok(values);
        }
        let requests = calls
            .into_iter()
            .map(|(method, params)| self.request(method, params))
            .collect::<Result<Vec<_>>>()?;
        self.dispatch(requests, true).await
    }
    pub async fn validate_chain(&self) -> Result<()> {
        let info = self.call("getblockchaininfo", json!([])).await?;
        ensure!(info["chain"] == "main", "Bitcoin RPC must use mainnet");
        ensure!(
            self.block_hash(0).await? == crate::GENESIS,
            "Bitcoin RPC genesis mismatch"
        );
        Ok(())
    }
    pub async fn height(&self) -> Result<u64> {
        self.call("getblockcount", json!([]))
            .await?
            .as_u64()
            .context("invalid RPC height")
    }
    pub async fn block_hash(&self, height: u64) -> Result<String> {
        model::hash(
            self.call("getblockhash", json!([height]))
                .await?
                .as_str()
                .context("invalid RPC block hash")?,
        )
    }
    pub async fn block(&self, hash: &str, verbosity: u64) -> Result<Value> {
        self.call("getblock", json!([hash, verbosity])).await
    }
}
fn decode(value: Value, ids: &[u64], batch: bool) -> std::result::Result<Vec<Value>, Failure> {
    let envelopes = if batch {
        value
            .as_array()
            .cloned()
            .ok_or_else(|| failure("invalid batch envelope", false))?
    } else {
        vec![value]
    };
    if envelopes.len() != ids.len() {
        return Err(failure("missing/extra batch responses", false));
    }
    let expected: HashSet<u64> = ids.iter().copied().collect();
    let mut values = HashMap::new();
    for row in envelopes {
        let id = row["id"]
            .as_u64()
            .filter(|id| expected.contains(id))
            .ok_or_else(|| failure("response identity mismatch", false))?;
        if values.contains_key(&id) {
            return Err(failure("duplicate response identity", false));
        }
        if !row["error"].is_null() {
            return Err(failure(
                "method failed",
                matches!(row["error"]["code"].as_i64(), Some(-28)),
            ));
        }
        let result = row
            .get("result")
            .cloned()
            .ok_or_else(|| failure("missing result", false))?;
        values.insert(id, result);
    }
    ids.iter()
        .map(|id| {
            values
                .remove(id)
                .ok_or_else(|| failure("missing response identity", false))
        })
        .collect()
}

#[derive(Clone)]
pub struct RpcPool {
    pub(crate) endpoints: Vec<Rpc>,
}
impl RpcPool {
    pub fn single(rpc: Rpc) -> Self {
        Self {
            endpoints: vec![rpc],
        }
    }
    pub fn new(endpoints: Vec<Rpc>) -> Result<Self> {
        ensure!(
            !endpoints.is_empty() && endpoints.len() <= 5,
            "configure 1..5 Bitcoin RPC endpoints"
        );
        let mut seen = HashSet::new();
        ensure!(
            endpoints.iter().all(|r| seen.insert(r.url.clone())),
            "duplicate Bitcoin RPC endpoints"
        );
        Ok(Self { endpoints })
    }
    pub fn from_env() -> Result<Self> {
        let primary = Rpc::from_env()?;
        let mut endpoints = vec![primary.clone()];
        let fallback = config::setting("BITCOIN_RPC_FALLBACK_URLS", "");
        if !fallback.trim().is_empty() {
            for url in fallback.split(',') {
                let mut rpc = primary.clone();
                rpc.url = config::origin(url.trim())?;
                rpc.sequence = Arc::new(AtomicU64::new(1));
                rpc.next_request = Arc::new(Mutex::new(Instant::now()));
                endpoints.push(rpc);
            }
        }
        Self::new(endpoints)
    }
}
