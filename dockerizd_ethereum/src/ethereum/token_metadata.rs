use std::{
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::primitives::{Address, U256, keccak256};
use anyhow::{Context, anyhow};
use tokio::time::sleep;

use crate::{
    config::AppConfig,
    storage::{EthereumStore, TokenMetadataJobRow, TokenMetadataRow},
};

use super::EthereumRpc;

const NAME_SELECTOR: [u8; 4] = [0x06, 0xfd, 0xde, 0x03];
const SYMBOL_SELECTOR: [u8; 4] = [0x95, 0xd8, 0x9b, 0x41];
const DECIMALS_SELECTOR: [u8; 4] = [0x31, 0x3c, 0xe5, 0x67];
const TOTAL_SUPPLY_SELECTOR: [u8; 4] = [0x18, 0x16, 0x0d, 0xdd];
const SUPPORTS_INTERFACE_SELECTOR: [u8; 4] = [0x01, 0xff, 0xc9, 0xa7];
const ERC721_INTERFACE: [u8; 4] = [0x80, 0xac, 0x58, 0xcd];
const ERC1155_INTERFACE: [u8; 4] = [0xd9, 0xb6, 0x7a, 0x26];

pub async fn run_token_metadata_worker(config: AppConfig, once: bool) -> anyhow::Result<()> {
    let rpc = EthereumRpc::connect(&config).await?;
    let store = EthereumStore::new(&config);

    loop {
        let jobs = store
            .pending_token_metadata_jobs(
                config.eth_token_metadata_batch_size,
                config.eth_token_metadata_max_attempts,
            )
            .await?;
        let job_count = jobs.len();

        for job in jobs {
            let now = unix_time_millis()?;
            match resolve_metadata(&config, &rpc, &job, now).await {
                Ok(metadata) => {
                    let completed = TokenMetadataJobRow {
                        status: "completed".to_string(),
                        attempt_count: job.attempt_count.saturating_add(1),
                        last_error: String::new(),
                        updated_at_unix_ms: now,
                        ..job
                    };
                    store.persist_token_metadata(&metadata, &completed).await?;
                    tracing::info!(
                        token_address = %metadata.token_address,
                        token_standard = %metadata.token_standard,
                        metadata_status = %metadata.metadata_status,
                        "Ethereum token metadata resolved"
                    );
                }
                Err(error) => {
                    let attempt_count = job.attempt_count.saturating_add(1);
                    let status = if attempt_count >= config.eth_token_metadata_max_attempts {
                        "failed"
                    } else {
                        "retry"
                    };
                    let failed = TokenMetadataJobRow {
                        status: status.to_string(),
                        attempt_count,
                        last_error: format!("{error:#}"),
                        updated_at_unix_ms: now,
                        ..job
                    };
                    store.update_token_metadata_job(&failed).await?;
                    tracing::warn!(
                        token_address = %failed.token_address,
                        attempt_count,
                        status,
                        error = %error,
                        "Ethereum token metadata resolution failed"
                    );
                }
            }
        }

        if once {
            tracing::info!(
                processed_jobs = job_count,
                "Ethereum token metadata batch complete"
            );
            return Ok(());
        }
        if job_count == 0 {
            sleep(Duration::from_secs(config.eth_token_metadata_poll_seconds)).await;
        }
    }
}

async fn resolve_metadata(
    config: &AppConfig,
    rpc: &EthereumRpc,
    job: &TokenMetadataJobRow,
    created_at_unix_ms: u64,
) -> anyhow::Result<TokenMetadataRow> {
    let address = Address::from_str(&job.token_address)
        .with_context(|| format!("invalid token address {}", job.token_address))?;
    let code = rpc.code_at(address).await?;
    if code.is_empty() {
        return Err(anyhow!("token address has no deployed bytecode"));
    }

    let token_standard = resolve_standard(rpc, address, &job.token_standard).await;
    let name = optional_call(rpc, address, &NAME_SELECTOR)
        .await
        .and_then(|value| decode_abi_string(&value))
        .unwrap_or_default();
    let symbol = optional_call(rpc, address, &SYMBOL_SELECTOR)
        .await
        .and_then(|value| decode_abi_string(&value))
        .unwrap_or_default();
    let decimals = optional_call(rpc, address, &DECIMALS_SELECTOR)
        .await
        .and_then(|value| decode_abi_u8(&value));
    let total_supply = optional_call(rpc, address, &TOTAL_SUPPLY_SELECTOR)
        .await
        .and_then(|value| decode_abi_u256(&value))
        .map(|value| value.to_string())
        .unwrap_or_default();

    let metadata_complete =
        !name.is_empty() && !symbol.is_empty() && (token_standard != "erc20" || decimals.is_some());

    Ok(TokenMetadataRow {
        network_id: config.eth_network_id.clone(),
        token_address: format!("{address:#x}"),
        token_standard,
        name,
        symbol,
        decimals: decimals.unwrap_or_default(),
        decimals_known: u8::from(decimals.is_some()),
        total_supply,
        code_hash: format!("{:#x}", keccak256(code)),
        is_verified: 0,
        metadata_source: "onchain_rpc".to_string(),
        metadata_status: if metadata_complete {
            "complete".to_string()
        } else {
            "partial".to_string()
        },
        created_at_unix_ms,
    })
}

async fn resolve_standard(rpc: &EthereumRpc, address: Address, discovered: &str) -> String {
    let normalized = discovered.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "erc20" | "erc721" | "erc1155") {
        return normalized;
    }
    if supports_interface(rpc, address, ERC1155_INTERFACE).await {
        "erc1155".to_string()
    } else if supports_interface(rpc, address, ERC721_INTERFACE).await {
        "erc721".to_string()
    } else {
        "erc20".to_string()
    }
}

async fn supports_interface(rpc: &EthereumRpc, address: Address, interface_id: [u8; 4]) -> bool {
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&SUPPORTS_INTERFACE_SELECTOR);
    calldata.extend_from_slice(&[0_u8; 28]);
    calldata.extend_from_slice(&interface_id);
    optional_call(rpc, address, &calldata)
        .await
        .and_then(|value| value.last().copied())
        == Some(1)
}

async fn optional_call(rpc: &EthereumRpc, address: Address, calldata: &[u8]) -> Option<Vec<u8>> {
    match rpc.call_contract(address, calldata).await {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) => None,
        Err(error) => {
            tracing::debug!(%address, error = %error, "optional token metadata call failed");
            None
        }
    }
}

fn decode_abi_string(data: &[u8]) -> Option<String> {
    let bytes = if data.len() == 32 {
        data
    } else {
        let offset = word_usize(data.get(..32)?)?;
        let length = word_usize(data.get(offset..offset.checked_add(32)?)?)?;
        let start = offset.checked_add(32)?;
        data.get(start..start.checked_add(length)?)?
    };
    let value = String::from_utf8_lossy(bytes)
        .trim_matches(char::from(0))
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect::<String>();
    (!value.is_empty()).then_some(value)
}

fn decode_abi_u8(data: &[u8]) -> Option<u8> {
    let word = data.get(..32)?;
    if word[..31].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(word[31])
}

fn decode_abi_u256(data: &[u8]) -> Option<U256> {
    Some(U256::from_be_slice(data.get(..32)?))
}

fn word_usize(word: &[u8]) -> Option<usize> {
    if word.len() != 32 || word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    let mut tail = [0_u8; 8];
    tail.copy_from_slice(&word[24..]);
    usize::try_from(u64::from_be_bytes(tail)).ok()
}

fn unix_time_millis() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis()
        .try_into()
        .context("Unix timestamp does not fit UInt64")?)
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U256;

    use super::{decode_abi_string, decode_abi_u8};

    #[test]
    fn decodes_dynamic_abi_string() {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&U256::from(32).to_be_bytes::<32>());
        encoded.extend_from_slice(&U256::from(4).to_be_bytes::<32>());
        encoded.extend_from_slice(b"USDC");
        encoded.extend_from_slice(&[0_u8; 28]);
        assert_eq!(decode_abi_string(&encoded).as_deref(), Some("USDC"));
    }

    #[test]
    fn distinguishes_known_decimals_from_missing_data() {
        let mut encoded = [0_u8; 32];
        encoded[31] = 18;
        assert_eq!(decode_abi_u8(&encoded), Some(18));
        assert_eq!(decode_abi_u8(&[]), None);
    }
}
