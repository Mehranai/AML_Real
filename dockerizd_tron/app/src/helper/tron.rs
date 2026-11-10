use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::sleep;

// ارتباط سطح پایین با Tron RPC را مدیریت میکند
const TRON_RPC_MAX_ATTEMPTS: usize = 5;
const TRON_RPC_MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(120);
// Full mainnet genesis ID specified in tronprotocol/TIPs tip-474.
pub const MAINNET_GENESIS: &str =
    "00000000000000001ebf88508a03865c71d452e25f4d51194196a1d22b6653dc";

#[derive(Debug)]
pub struct RpcHttpError {
    pub status: u16,
    retry_after: Option<u64>,
}
impl std::fmt::Display for RpcHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TRON RPC HTTP status {}", self.status)
    }
}
impl std::error::Error for RpcHttpError {}
pub fn transient_rpc_error(error: &anyhow::Error) -> bool {
    if let Some(http) = error.downcast_ref::<RpcHttpError>() {
        return matches!(http.status, 408 | 425 | 429 | 500..=599);
    }
    error.downcast_ref::<reqwest::Error>().is_some_and(|e| {
        e.is_timeout() || e.is_connect() || e.is_body() || (e.is_request() && !e.is_builder())
    })
}

#[derive(Clone)]
pub struct TronClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    last_request_at: Arc<Mutex<Instant>>,
}

impl TronClient {
    pub async fn validate_mainnet(&self) -> Result<()> {
        let genesis = self.get_block(0).await?;
        validate_genesis(&genesis)
    }
    pub fn new(base_url: &str, api_key: Option<String>, timeout_seconds: u64) -> Result<Self> {
        let timeout = Duration::from_secs(timeout_seconds.max(1));

        let client = Client::builder()
            .timeout(timeout)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .context("Failed to build reqwest client")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            last_request_at: Arc::new(Mutex::new(
                Instant::now()
                    .checked_sub(TRON_RPC_MIN_REQUEST_INTERVAL)
                    .unwrap_or_else(Instant::now),
            )),
        })
    }

    pub async fn post(&self, endpoint: &str, body: Value) -> Result<Value> {
        let mut last_error = None;

        for attempt in 1..=TRON_RPC_MAX_ATTEMPTS {
            match self.post_once(endpoint, body.clone()).await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if !transient_rpc_error(&err) {
                        return Err(err);
                    }
                    if attempt < TRON_RPC_MAX_ATTEMPTS {
                        let delay = retry_delay(&err, attempt);

                        eprintln!(
                            "[TRON RPC] endpoint={} attempt {}/{} failed; retrying in {:?}: {}",
                            endpoint, attempt, TRON_RPC_MAX_ATTEMPTS, delay, err
                        );

                        sleep(delay).await;
                    }

                    last_error = Some(err);
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| anyhow!("Tron API request failed without an error"))
            .context(format!(
                "Tron API endpoint {} failed after {} attempts",
                endpoint, TRON_RPC_MAX_ATTEMPTS
            )))
    }

    async fn post_once(&self, endpoint: &str, body: Value) -> Result<Value> {
        self.wait_for_rate_limit_slot().await;

        let url = format!("{}/{}", self.base_url, endpoint);

        let mut request = self.client.post(&url).json(&body);

        if let Some(key) = &self.api_key {
            request = request.header("TRON-PRO-API-KEY", key);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("HTTP request failed: {}", endpoint))?;

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        // let text = response.text().await?;
        let text = response
            .text()
            .await
            .with_context(|| format!("Failed reading body from endpoint: {}", endpoint))?;

        if !status.is_success() {
            return Err(RpcHttpError {
                status: status.as_u16(),
                retry_after: retry_after.or_else(|| parse_suspended_seconds(&text)),
            }
            .into());
        }

        let parsed: Value = serde_json::from_str(&text)
            .with_context(|| format!("Invalid JSON from endpoint: {}", endpoint))?;

        // TronGrid can report throttling in a successful HTTP response.
        if let Some(message) = parsed
            .get("Error")
            .or_else(|| parsed.get("error"))
            .and_then(Value::as_str)
        {
            let lower = message.to_ascii_lowercase();
            if lower.contains("frequency limit") || lower.contains("rate limit") {
                return Err(RpcHttpError {
                    status: 429,
                    retry_after: parse_suspended_seconds(message),
                }
                .into());
            }
        }

        Ok(parsed)
    }

    async fn wait_for_rate_limit_slot(&self) {
        let mut last_request_at = self.last_request_at.lock().await;
        let elapsed = last_request_at.elapsed();

        if elapsed < TRON_RPC_MIN_REQUEST_INTERVAL {
            sleep(TRON_RPC_MIN_REQUEST_INTERVAL - elapsed).await;
        }

        *last_request_at = Instant::now();
    }

    // --------------------------------------------------
    // PUBLIC API METHODS
    // --------------------------------------------------

    pub async fn get_block(&self, number: u64) -> Result<Value> {
        self.post("wallet/getblockbynum", serde_json::json!({ "num": number }))
            .await
    }

    pub async fn get_now_block(&self) -> Result<Value> {
        self.post("wallet/getnowblock", serde_json::json!({})).await
    }

    pub async fn get_now_solid_block(&self) -> Result<Value> {
        self.post("walletsolidity/getnowblock", serde_json::json!({}))
            .await
    }

    pub async fn get_tx_receipt(&self, tx_hash: &str) -> Result<Value> {
        self.post(
            "wallet/gettransactioninfobyid",
            serde_json::json!({ "value": tx_hash }),
        )
        .await
    }

    pub async fn get_account(&self, address: &str) -> Result<Value> {
        self.post(
            "wallet/getaccount",
            serde_json::json!({
                "address": address,
                "visible": true
            }),
        )
        .await
    }

    pub async fn get_block_number(&self) -> Result<u64> {
        let block = self.get_now_block().await?;

        block["block_header"]["raw_data"]["number"]
            .as_u64()
            .ok_or_else(|| anyhow!("Failed to parse block number"))
    }

    pub async fn get_solid_block_number(&self) -> Result<u64> {
        let block = self.get_now_solid_block().await?;

        block["block_header"]["raw_data"]["number"]
            .as_u64()
            .ok_or_else(|| anyhow!("Failed to parse solid block number"))
    }
}

fn validate_genesis(genesis: &Value) -> Result<()> {
    let header = genesis
        .pointer("/block_header/raw_data")
        .and_then(Value::as_object)
        .context("TRON source validation failed: genesis header missing")?;
    anyhow::ensure!(
        genesis["blockID"]
            .as_str()
            .is_some_and(|id| id.eq_ignore_ascii_case(MAINNET_GENESIS))
            && header.get("number").is_none_or(|n| n.as_u64() == Some(0)),
        "TRON source validation failed: RPC genesis is not mainnet"
    );
    Ok(())
}
fn retry_delay(err: &anyhow::Error, attempt: usize) -> Duration {
    if let Some(http) = err.downcast_ref::<RpcHttpError>() {
        if let Some(seconds) = http.retry_after {
            return Duration::from_secs(seconds.saturating_add(1).min(60));
        }
        if http.status == 429 {
            return Duration::from_secs(30);
        }
    }
    Duration::from_millis(500 * attempt as u64)
}

fn parse_suspended_seconds(message: &str) -> Option<u64> {
    let marker = "suspended for ";
    let start = message.find(marker)? + marker.len();
    let tail = &message[start..];
    let digits = tail
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();

    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mainnet_genesis_accepts_only_the_proto3_zero_default() {
        let mut genesis = serde_json::json!({"blockID":MAINNET_GENESIS.to_ascii_uppercase(),"block_header":{"raw_data":{}}});
        assert!(validate_genesis(&genesis).is_ok());
        genesis["block_header"]["raw_data"]["number"] = serde_json::json!(0);
        assert!(validate_genesis(&genesis).is_ok());
        genesis["block_header"]["raw_data"]["number"] = serde_json::json!(1);
        assert!(validate_genesis(&genesis).is_err());
        genesis["block_header"]["raw_data"]["number"] = serde_json::json!(0);
        genesis["blockID"] = serde_json::json!("f".repeat(64));
        assert!(validate_genesis(&genesis).is_err());
        assert!(validate_genesis(&serde_json::json!({"blockID":MAINNET_GENESIS})).is_err());
    }
    #[test]
    fn rpc_retry_classification_uses_status_and_bounded_hints() {
        for status in [400, 401, 403, 404, 408, 425, 429, 500, 502, 503, 504] {
            let error: anyhow::Error = RpcHttpError {
                status,
                retry_after: Some(u64::MAX),
            }
            .into();
            assert_eq!(
                transient_rpc_error(&error),
                matches!(status, 408 | 425 | 429 | 500..=599)
            );
            assert!(retry_delay(&error, 1) <= Duration::from_secs(60));
            assert_eq!(error.to_string(), format!("TRON RPC HTTP status {status}"));
        }
        assert!(!transient_rpc_error(&anyhow!(
            "invalid source data mentioning timeout 503"
        )));
    }

    #[tokio::test]
    async fn authentication_failure_is_not_retried_or_logged_with_its_body() {
        use axum::{Router, http::StatusCode, routing::post};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let requests = calls.clone();
        let app=Router::new().route("/walletsolidity/getnowblock",post(|| async {axum::Json(serde_json::json!({"Error":"The key exceeds the frequency limit, suspended for 999999 s"}))}))
        .route("/wallet/getnowblock",post(move || {let requests=requests.clone();async move {
            requests.fetch_add(1,Ordering::SeqCst);(StatusCode::UNAUTHORIZED,"private-fixture-key")
        }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = TronClient::new(&url, None, 2).unwrap();
        let error = client.get_now_block().await.unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!format!("{error:#}").contains("private-fixture-key"));
        let limited = client
            .post_once("walletsolidity/getnowblock", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(transient_rpc_error(&limited));
        assert_eq!(retry_delay(&limited, 1), Duration::from_secs(60));
        server.abort();
    }

    #[test]
    fn parses_trongrid_suspension_window() {
        let message = r#"{"Error":"The key exceeds the frequency limit(15), and the query server is suspended for 30 s"}"#;

        assert_eq!(parse_suspended_seconds(message), Some(30));
    }
}
