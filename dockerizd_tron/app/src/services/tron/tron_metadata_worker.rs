//! Metadata observations at a bracketed solid RPC head, never backdated to discovery.
use crate::{
    config::AppConfig,
    db::{Client, now_ms, records::operational_lock, row},
    helper::tron::TronClient,
};
use aml_warehouse::{Network, visibility};
use anyhow::{Context, Result, ensure};
use clickhouse::types::UInt256;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
const METADATA_OWNER_ADDRESS: &str = "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb";
#[derive(Deserialize, clickhouse::Row)]
struct Job {
    asset_id: String,
    discovery_id: String,
    discovered_block: u64,
    token_address: String,
}
pub async fn run_token_metadata_worker(config: AppConfig) -> Result<()> {
    let client = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass)
        .with_database(&config.clickhouse_db_tron);
    let rpc = TronClient::new(
        config
            .tron_rpc_url
            .as_deref()
            .context("TRON_RPC_URL required")?,
        config.tron_api_key.clone(),
        config.rpc_timeout_seconds,
    )?;
    loop {
        if let Err(error) = worker_once(
            &client,
            &rpc,
            config.tron_metadata_batch_size,
            config.tron_metadata_max_attempts,
        )
        .await
        {
            eprintln!("[TRON METADATA] cycle failed: {error:#}");
        }
        tokio::time::sleep(Duration::from_secs(
            config.tron_metadata_poll_interval_seconds.max(1),
        ))
        .await;
    }
}
pub async fn worker_once(
    client: &Client,
    rpc: &TronClient,
    batch_size: u64,
    max_attempts: u8,
) -> Result<usize> {
    ensure!(max_attempts > 0, "metadata max attempts must be positive");
    let _lock = operational_lock(client)?;
    let snapshot = visibility::capture(client, client.database(), Network::Tron).await?;
    if snapshot.state().initialized == 0 {
        return Ok(0);
    }
    let jobs=client.query("SELECT asset_id,argMin(discovery_id,(block_number,transaction_index)) AS discovery_id,min(block_number) AS discovered_block,argMin(JSONExtractString(raw_evidence_json,'token_address'),(block_number,transaction_index)) AS token_address FROM aml_metadata_discoveries_canonical WHERE network_id='tron:mainnet' AND standard_hint='trc20' AND asset_id NOT IN (SELECT asset_id FROM aml_asset_metadata_visible WHERE network_id='tron:mainnet' AND metadata_status='complete') AND asset_id NOT IN (SELECT asset_id FROM aml_metadata_jobs_visible WHERE network_id='tron:mainnet' AND (attempts>=? OR next_attempt_unix_ms>?)) GROUP BY asset_id ORDER BY discovered_block,asset_id LIMIT ?")
        .bind(u32::from(max_attempts)).bind(now_ms()).bind(batch_size.clamp(1,100)).fetch_all::<Job>().await?;
    let count = jobs.len();
    for job in jobs {
        let prior = client
            .rows_from(
                "aml_metadata_jobs",
                "visible",
                &format!(
                    "asset_id={}",
                    aml_warehouse::publication::literal(&job.asset_id)
                ),
            )
            .await?;
        let attempts = prior
            .first()
            .and_then(|v| v["attempts"].as_u64())
            .unwrap_or(0)
            + 1;
        write_job(client, &job, &snapshot, attempts, "running", "").await?;
        match observe(client, rpc, &job, &snapshot).await {
            Ok(metadata) => {
                visibility::unchanged(client, client.database(), &snapshot).await?;
                client
                    .insert_rows("aml_asset_metadata", &[metadata])
                    .await?;
                write_job(client, &job, &snapshot, attempts, "complete", "").await?;
            }
            Err(error) => {
                let status = if attempts >= u64::from(max_attempts) {
                    "dead_letter"
                } else {
                    "retry"
                };
                write_job(
                    client,
                    &job,
                    &snapshot,
                    attempts,
                    status,
                    &format!("{error:#}"),
                )
                .await?;
            }
        }
    }
    Ok(count)
}
async fn write_job(
    client: &Client,
    job: &Job,
    snapshot: &visibility::Snapshot,
    attempts: u64,
    status: &str,
    error: &str,
) -> Result<()> {
    let now = now_ms();
    let value = row(
        "aml_metadata_jobs",
        json!({"job_id":format!("tron:metadata:{}",job.asset_id),"asset_id":job.asset_id,"status":status,"attempts":attempts,"next_attempt_unix_ms":if status=="retry" {now+30000} else {0},"last_error":truncate_error(error),"discovery_id":job.discovery_id,"discovered_block":job.discovered_block,"source_revision":snapshot.state().revision,"updated_at_unix_ms":now,"started_at_unix_ms":now,"completed_at_unix_ms":if status=="complete" {json!(now)} else {Value::Null},"raw_evidence_json":json!({"token_address":job.token_address}).to_string()}),
        client.revision("aml_metadata_jobs").await?,
        now,
    )?;
    client.insert_rows("aml_metadata_jobs", &[value]).await
}
async fn head(rpc: &TronClient) -> Result<(u64, String, Value)> {
    let value = rpc.get_now_solid_block().await?;
    let height = value["block_header"]["raw_data"]["number"]
        .as_u64()
        .context("solid RPC height missing")?;
    let hash = aml_warehouse::values::normalize_hash(
        Network::Tron,
        value["blockID"]
            .as_str()
            .context("solid RPC hash missing")?,
    )?;
    Ok((height, hash, value))
}
async fn observe(
    client: &Client,
    rpc: &TronClient,
    job: &Job,
    snapshot: &visibility::Snapshot,
) -> Result<Value> {
    let address = aml_warehouse::values::tron_wallet(&job.token_address)?;
    let (height, hash, head_before) = head(rpc).await?;
    ensure!(
        height >= job.discovered_block && height <= snapshot.state().last_block_number,
        "solid metadata head is outside indexed canonical history; wait for ingestion"
    );
    let expected=client.query("SELECT block_hash FROM aml_ingested_blocks_canonical WHERE network_id='tron:mainnet' AND block_number=?").bind(height).fetch_optional::<String>().await?.context("metadata observation block not indexed")?;
    ensure!(
        expected == hash,
        "RPC metadata head differs from indexed canonical block"
    );
    let mut calls = Vec::new();
    let symbol = call_constant(rpc, &address, "symbol()", &mut calls).await?;
    let name = call_constant(rpc, &address, "name()", &mut calls).await?;
    let decimals = call_constant(rpc, &address, "decimals()", &mut calls).await?;
    let decimals =
        u8::try_from(decode_abi_uint256(&decimals)?).context("decimals exceeds UInt8")?;
    let symbol = abi_text(&symbol)?;
    let name = abi_text(&name)?;
    let (after_height, after_hash, head_after) = head(rpc).await?;
    ensure!(
        height == after_height && hash == after_hash,
        "solid state changed during metadata calls; retry"
    );
    visibility::unchanged(client, client.database(), snapshot).await?;
    row(
        "aml_asset_metadata",
        json!({"asset_id":job.asset_id,"symbol":symbol,"name":name,"decimals":decimals,"asset_standard":"trc20","source_id":"tron_solidity_rpc","source_reference":format!("block:{hash}"),"review_status":"pending","reviewed_by":"","evidence_refs":[format!("block:{hash}"),format!("discovery:{}",job.discovery_id)],"is_verified":0,"metadata_status":"complete","observed_block":height,"observed_block_hash":hash,"source_revision":snapshot.state().revision,"updated_at_unix_ms":now_ms(),"raw_evidence_json":json!({"token_address":address,"head_before":head_before,"head_after":head_after,"calls":calls,"state_basis":"latest_solid_state_bracketed_not_historical_query"}).to_string()}),
        client.revision("aml_asset_metadata").await?,
        now_ms(),
    )
}
async fn call_constant(
    rpc: &TronClient,
    address: &str,
    selector: &str,
    calls: &mut Vec<Value>,
) -> Result<Vec<u8>> {
    let response=rpc.post("walletsolidity/triggerconstantcontract",json!({"owner_address":METADATA_OWNER_ADDRESS,"contract_address":address,"function_selector":selector,"parameter":"","visible":true})).await?;
    ensure!(
        constant_succeeded(&response),
        "constant call failed: {selector}"
    );
    let raw = response["constant_result"][0]
        .as_str()
        .context("missing constant result")?;
    ensure!(
        raw.len() <= 8192 && raw.len() % 2 == 0,
        "invalid metadata ABI size"
    );
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))?;
    calls.push(json!({"function_selector":selector,"response":response}));
    Ok(bytes)
}
fn constant_succeeded(response: &Value) -> bool {
    response["result"]["result"] == true
        && response["transaction"]["ret"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|entry| {
                ["ret", "contractRet"]
                    .iter()
                    .all(|key| entry[*key].as_str().is_none_or(|value| value == "SUCCESS"))
            })
}
fn abi_text(bytes: &[u8]) -> Result<String> {
    let value = if bytes.len() == 32 {
        std::str::from_utf8(bytes)?
            .trim_end_matches('\0')
            .to_owned()
    } else {
        decode_dynamic_abi_string(bytes).context("invalid dynamic ABI text")?
    };
    ensure!(
        value.chars().count() <= 128 && !value.chars().any(char::is_control),
        "invalid metadata text"
    );
    Ok(sanitize_metadata_text(&value))
}
fn decode_dynamic_abi_string(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 64
        || bytes[..31].iter().any(|b| *b != 0)
        || bytes[31] != 32
        || bytes[32..56].iter().any(|b| *b != 0)
    {
        return None;
    }
    let len = u64::from_be_bytes(bytes[56..64].try_into().ok()?) as usize;
    if len > 512 || 64usize.checked_add(len)? > bytes.len() {
        return None;
    }
    std::str::from_utf8(&bytes[64..64 + len])
        .ok()
        .map(str::to_owned)
}
fn decode_abi_uint256(bytes: &[u8]) -> Result<UInt256> {
    ensure!(bytes.len() == 32, "ABI uint256 requires exactly one word");
    let mut word = [0u8; 32];
    for (i, b) in bytes.iter().rev().enumerate() {
        word[i] = *b;
    }
    Ok(UInt256::from_le_bytes(word))
}
fn sanitize_metadata_text(value: &str) -> String {
    value.trim_matches('\0').trim().chars().take(128).collect()
}
fn truncate_error(error: &str) -> String {
    error.chars().take(1000).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        decode_abi_uint256, decode_dynamic_abi_string, sanitize_metadata_text, truncate_error,
    };

    #[test]
    fn explicit_simulation_failure_wins_over_http_success_and_data() {
        use serde_json::json;
        assert!(super::constant_succeeded(
            &json!({"result":{"result":true}})
        ));
        for key in ["ret", "contractRet"] {
            let mut response = json!({"result":{"result":true},"constant_result":["00"],"transaction":{"ret":[{}]}});
            response["transaction"]["ret"][0][key] = json!("REVERT");
            assert!(!super::constant_succeeded(&response));
        }
    }
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
