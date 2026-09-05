use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use clickhouse::{Client, types::UInt256};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::config::AppConfig;
use crate::helper::tron::TronClient;
use crate::models::token_metadata::TokenMetadataRow;
use crate::progress::core::save_token_metadata;

const METADATA_OWNER_ADDRESS: &str = "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb";

#[derive(Debug, Deserialize, clickhouse::Row)]
struct PendingMetadataJob {
    token_address: String,
    attempt_count: u8,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
struct TokenMetadataJobState {
    token_address: String,
    status: String,
    attempt_count: u8,
    last_error: String,
    updated_at_unix_ms: u64,
}

pub async fn run_token_metadata_worker(config: AppConfig) -> Result<()> {
    let clickhouse = Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron),
    );
    let rpc_url = config
        .tron_rpc_url
        .as_deref()
        .ok_or_else(|| anyhow!("TRON_RPC_URL or TRON_RPC_HTTP must be configured"))?;
    let tron_client = Arc::new(TronClient::new(
        rpc_url,
        config.tron_api_key.clone(),
        config.rpc_timeout_seconds,
    )?);
    let poll_interval = Duration::from_secs(config.tron_metadata_poll_interval_seconds.max(1));

    println!(
        "[TRON METADATA] worker started (batch_size={}, max_attempts={})",
        config.tron_metadata_batch_size, config.tron_metadata_max_attempts
    );

    loop {
        let jobs = load_pending_jobs(
            &clickhouse,
            config.tron_metadata_batch_size,
            config.tron_metadata_max_attempts,
        )
        .await?;

        if jobs.is_empty() {
            sleep(poll_interval).await;
            continue;
        }

        for job in jobs {
            process_metadata_job(
                clickhouse.clone(),
                tron_client.clone(),
                job,
                config.tron_metadata_max_attempts,
            )
            .await;
        }
    }
}

async fn load_pending_jobs(
    clickhouse: &Client,
    batch_size: u64,
    max_attempts: u8,
) -> Result<Vec<PendingMetadataJob>> {
    clickhouse
        .query(
            r#"
            SELECT
                discovery.token_address AS token_address,
                if(job.token_address = '', toUInt8(0), job.attempt_count) AS attempt_count
            FROM token_metadata_discoveries AS discovery FINAL
            LEFT JOIN token_metadata AS metadata FINAL
                ON metadata.token_address = discovery.token_address
            LEFT JOIN token_metadata_jobs AS job FINAL
                ON job.token_address = discovery.token_address
            WHERE metadata.token_address = ''
              AND (
                    job.token_address = ''
                    OR (job.status = 'RETRY' AND job.attempt_count < ?)
                  )
            ORDER BY discovery.discovered_block, discovery.token_address
            LIMIT ?
            "#,
        )
        .bind(max_attempts)
        .bind(batch_size.max(1))
        .fetch_all::<PendingMetadataJob>()
        .await
        .context("failed to load pending TRON token metadata jobs")
}

async fn process_metadata_job(
    clickhouse: Arc<Client>,
    tron_client: Arc<TronClient>,
    job: PendingMetadataJob,
    max_attempts: u8,
) {
    let attempt_count = job.attempt_count.saturating_add(1);

    match fetch_token_metadata(&tron_client, &job.token_address).await {
        Ok(metadata) => {
            if let Err(error) = save_token_metadata(clickhouse.clone(), metadata).await {
                record_metadata_failure(
                    &clickhouse,
                    &job,
                    attempt_count,
                    max_attempts,
                    format!("failed to persist metadata: {error:#}"),
                )
                .await;
                return;
            }

            if let Err(error) = write_job_state(
                &clickhouse,
                TokenMetadataJobState {
                    token_address: job.token_address.clone(),
                    status: "COMPLETE".to_string(),
                    attempt_count,
                    last_error: String::new(),
                    updated_at_unix_ms: now_unix_ms(),
                },
            )
            .await
            {
                eprintln!(
                    "[TRON METADATA] failed to mark {} complete: {error:#}",
                    job.token_address
                );
            } else {
                println!(
                    "[TRON METADATA] resolved {} on attempt {}",
                    job.token_address, attempt_count
                );
            }
        }
        Err(error) => {
            record_metadata_failure(
                &clickhouse,
                &job,
                attempt_count,
                max_attempts,
                format!("{error:#}"),
            )
            .await;
        }
    }
}

async fn record_metadata_failure(
    clickhouse: &Client,
    job: &PendingMetadataJob,
    attempt_count: u8,
    max_attempts: u8,
    error: String,
) {
    let status = if attempt_count >= max_attempts {
        "FAILED"
    } else {
        "RETRY"
    };
    let last_error = truncate_error(&error);
    let result = write_job_state(
        clickhouse,
        TokenMetadataJobState {
            token_address: job.token_address.clone(),
            status: status.to_string(),
            attempt_count,
            last_error: last_error.clone(),
            updated_at_unix_ms: now_unix_ms(),
        },
    )
    .await;

    if let Err(write_error) = result {
        eprintln!(
            "[TRON METADATA] {} attempt {} failed ({last_error}); state write also failed: {write_error:#}",
            job.token_address, attempt_count
        );
    } else {
        eprintln!(
            "[TRON METADATA] {} attempt {} -> {}: {}",
            job.token_address, attempt_count, status, last_error
        );
    }
}

async fn write_job_state(clickhouse: &Client, state: TokenMetadataJobState) -> Result<()> {
    let mut insert = clickhouse
        .insert::<TokenMetadataJobState>("token_metadata_jobs")
        .await?;
    insert.write(&state).await?;
    insert.end().await?;
    Ok(())
}

pub async fn fetch_token_metadata(
    tron_client: &TronClient,
    token_address: &str,
) -> Result<TokenMetadataRow> {
    let symbol = call_string(tron_client, token_address, "symbol()")
        .await
        .context("symbol() failed")?;
    let name = call_string(tron_client, token_address, "name()")
        .await
        .unwrap_or_else(|_| symbol.clone());
    let decimals_value = call_uint(tron_client, token_address, "decimals()")
        .await
        .context("decimals() failed")?;
    let decimals = u8::try_from(decimals_value)
        .map_err(|_| anyhow!("decimals() returned an out-of-range value"))?;
    Ok(TokenMetadataRow {
        token_address: token_address.to_string(),
        name: sanitize_metadata_text(&name),
        symbol: sanitize_metadata_text(&symbol),
        decimals,
        is_verified: 0,
    })
}

async fn call_string(
    tron_client: &TronClient,
    contract_address: &str,
    function_selector: &str,
) -> Result<String> {
    let bytes = call_constant(tron_client, contract_address, function_selector).await?;

    if let Some(value) = decode_dynamic_abi_string(&bytes) {
        return Ok(value);
    }

    if let Some(word) = bytes.get(..32) {
        let value = String::from_utf8_lossy(word)
            .trim_matches('\0')
            .trim()
            .to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }

    Err(anyhow!(
        "{function_selector} did not return a supported ABI string"
    ))
}

async fn call_uint(
    tron_client: &TronClient,
    contract_address: &str,
    function_selector: &str,
) -> Result<UInt256> {
    let bytes = call_constant(tron_client, contract_address, function_selector).await?;
    decode_abi_uint256(&bytes)
        .with_context(|| format!("{function_selector} returned invalid ABI uint data"))
}

fn decode_dynamic_abi_string(bytes: &[u8]) -> Option<String> {
    let offset = abi_word_to_usize(bytes.get(..32)?)?;
    let length_word_end = offset.checked_add(32)?;
    let length = abi_word_to_usize(bytes.get(offset..length_word_end)?)?;
    let value_end = length_word_end.checked_add(length)?;
    let value = String::from_utf8_lossy(bytes.get(length_word_end..value_end)?)
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn abi_word_to_usize(word: &[u8]) -> Option<usize> {
    if word.len() != 32 || word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(u64::from_be_bytes(word[24..].try_into().ok()?) as usize)
}

fn decode_abi_uint256(bytes: &[u8]) -> Result<UInt256> {
    let word = bytes
        .get(..32)
        .ok_or_else(|| anyhow!("ABI uint256 requires a 32-byte word"))?;
    let mut little_endian = [0_u8; 32];
    for (index, byte) in word.iter().rev().enumerate() {
        little_endian[index] = *byte;
    }
    Ok(UInt256::from_le_bytes(little_endian))
}

async fn call_constant(
    tron_client: &TronClient,
    contract_address: &str,
    function_selector: &str,
) -> Result<Vec<u8>> {
    let response = tron_client
        .post(
            "wallet/triggerconstantcontract",
            serde_json::json!({
                "owner_address": METADATA_OWNER_ADDRESS,
                "contract_address": contract_address,
                "function_selector": function_selector,
                "parameter": "",
                "visible": true
            }),
        )
        .await?;

    if !response["result"]["result"].as_bool().unwrap_or(false) {
        return Err(anyhow!(
            "contract rejected {function_selector}: {}",
            response["result"]["message"]
                .as_str()
                .unwrap_or("unknown contract error")
        ));
    }

    let encoded = response["constant_result"][0]
        .as_str()
        .ok_or_else(|| anyhow!("{function_selector} returned no constant_result"))?;
    hex::decode(encoded.trim_start_matches("0x"))
        .with_context(|| format!("{function_selector} returned invalid hex"))
}

fn sanitize_metadata_text(value: &str) -> String {
    value.trim_matches('\0').trim().chars().take(128).collect()
}

fn truncate_error(error: &str) -> String {
    error.chars().take(1_000).collect()
}

fn now_unix_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::{
        decode_abi_uint256, decode_dynamic_abi_string, sanitize_metadata_text, truncate_error,
    };

    #[test]
    fn metadata_text_is_trimmed_and_bounded() {
        let value = format!("  USDT\0{}  ", "x".repeat(200));
        let sanitized = sanitize_metadata_text(&value);
        assert!(sanitized.starts_with("USDT"));
        assert!(sanitized.chars().count() <= 128);
    }

    #[test]
    fn job_errors_are_bounded() {
        assert_eq!(truncate_error(&"x".repeat(2_000)).len(), 1_000);
    }

    #[test]
    fn decodes_trc20_abi_values_without_an_evm_provider() {
        let mut encoded_string = vec![0_u8; 96];
        encoded_string[31] = 32;
        encoded_string[63] = 4;
        encoded_string[64..68].copy_from_slice(b"USDT");
        assert_eq!(
            decode_dynamic_abi_string(&encoded_string).as_deref(),
            Some("USDT")
        );

        let mut encoded_uint = [0_u8; 32];
        encoded_uint[31] = 6;
        assert_eq!(decode_abi_uint256(&encoded_uint).unwrap().to_string(), "6");
    }
}
