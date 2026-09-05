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

#[derive(Clone)]
pub struct TronClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    last_request_at: Arc<Mutex<Instant>>,
}

impl TronClient {
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
        // let text = response.text().await?;
        let text = response
            .text()
            .await
            .with_context(|| format!("Failed reading body from endpoint: {}", endpoint))?;

        if !status.is_success() {
            return Err(anyhow!(
                "Tron API error | endpoint={} | status={} | body={}",
                endpoint,
                status,
                text
            ));
        }

        let parsed: Value = serde_json::from_str(&text)
            .with_context(|| format!("Invalid JSON from endpoint: {}", endpoint))?;

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

fn retry_delay(err: &anyhow::Error, attempt: usize) -> Duration {
    let message = err.to_string();

    if message.contains("429 Too Many Requests") || message.contains("frequency limit") {
        if let Some(seconds) = parse_suspended_seconds(&message) {
            return Duration::from_secs(seconds.saturating_add(1));
        }

        return Duration::from_secs(30);
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
    use super::parse_suspended_seconds;

    #[test]
    fn parses_trongrid_suspension_window() {
        let message = r#"{"Error":"The key exceeds the frequency limit(15), and the query server is suspended for 30 s"}"#;

        assert_eq!(parse_suspended_seconds(message), Some(30));
    }
}
