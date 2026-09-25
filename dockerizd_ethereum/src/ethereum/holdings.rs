use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::primitives::{Address, U256};
use anyhow::{Context, Result, ensure};
use clickhouse::{Client, Row};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    sync::Semaphore,
    task::JoinSet,
    time::{Instant, timeout, timeout_at},
};

use super::EthereumRpc;
use crate::config::AppConfig;

static QUERIES: Semaphore = Semaphore::const_new(4);
const ASSET_LIMIT: usize = 100;

#[derive(Debug, Deserialize, Row)]
struct Asset {
    asset_id: String,
    token_id: String,
    symbol: String,
    decimals: Option<u8>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn unavailable(network: &str, address: &str, reason: &str) -> Value {
    json!({"network_id":network,"address":address,"status":"unavailable","error_class":reason,
        "source":"finalized_rpc","assets":[],"block_number":null,"block_hash":null,
        "scope":"native_and_discovered_assets","observation_scope":"current_finalized_not_historical_window",
        "observed_at_unix_ms":now_ms(),"unavailable_is_not_zero":true,"persisted":false})
}

pub async fn snapshot(config: &AppConfig, db: &Client, address: &str) -> Value {
    let Ok(_permit) = QUERIES.try_acquire() else {
        return unavailable(&config.eth_network_id, address, "capacity_reached");
    };
    match timeout(Duration::from_secs(35), load(config, db, address)).await {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => unavailable(
            &config.eth_network_id,
            address,
            "rpc_or_warehouse_unavailable",
        ),
        Err(_) => unavailable(&config.eth_network_id, address, "deadline_exceeded"),
    }
}

async fn load(config: &AppConfig, db: &Client, address: &str) -> Result<Value> {
    let wallet = Address::from_str(address).context("invalid wallet")?;
    let network = &config.eth_network_id;
    let native = format!("{network}/native:eth");
    let mut assets = db.query(r#"
        SELECT r.asset_id AS asset_id, r.token_id AS token_id, m.symbol AS symbol,
            if(m.decimals_known = 1, toNullable(m.decimals), NULL) AS decimals
        FROM (
            SELECT DISTINCT network_id, asset_id, token_id FROM address_relationships_canonical
            WHERE network_id = ? AND (from_address = ? OR to_address = ?) AND asset_id != ?
            ORDER BY asset_id, token_id LIMIT 101
        ) AS r
        LEFT ANY JOIN token_metadata_canonical AS m
            ON r.network_id = m.network_id AND arrayElement(splitByChar(':', r.asset_id), -1) = m.token_address
        ORDER BY asset_id, token_id
    "#).bind(network).bind(address).bind(address).bind(&native).fetch_all::<Asset>().await?;
    let truncated = assets.len() > ASSET_LIMIT;
    assets.truncate(ASSET_LIMIT);
    assets.insert(
        0,
        Asset {
            asset_id: native.clone(),
            token_id: String::new(),
            symbol: "ETH".into(),
            decimals: Some(18),
        },
    );

    let rpc = EthereumRpc::connect(config).await?;
    let (height, hash) = rpc.holdings_block().await?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let slots = Arc::new(Semaphore::new(4));
    let mut jobs = JoinSet::new();
    let mut rows = Vec::new();
    for (index, asset) in assets.into_iter().enumerate() {
        let nft = asset.asset_id.contains("/erc721:") || asset.asset_id.contains("/erc1155:");
        rows.push(
            json!({"asset_id":asset.asset_id,"token_id":asset.token_id,"symbol":asset.symbol,
            "decimals":if nft {Some(0)} else {asset.decimals},"amount":null,"status":"unavailable",
            "error_class":"deadline_exceeded"}),
        );
        let rpc = rpc.clone();
        let slots = slots.clone();
        let network = network.clone();
        let native = native.clone();
        jobs.spawn(async move {
            let result = async {
                let _permit = slots.acquire_owned().await?;
                if asset.asset_id == native {
                    Ok(rpc.balance_at_block(wallet, height).await?.to_string())
                } else {
                    let (contract, data, owner_of) = token_call(&network, &asset, wallet)?;
                    let bytes = rpc.call_contract_at_block(contract, &data, height).await?;
                    decode_balance(&bytes, owner_of, wallet)
                }
            };
            let result = match timeout_at(deadline, result).await {
                Ok(value) => value,
                Err(_) => Err(anyhow::anyhow!("deadline")),
            };
            (index, result)
        });
    }
    while let Some(result) = jobs.join_next().await {
        let (index, result) = result.context("holdings worker failed")?;
        match result {
            Ok(amount) => {
                rows[index]["amount"] = json!(amount);
                rows[index]["status"] = json!("available");
                rows[index]["error_class"] = Value::Null;
            }
            Err(_) => rows[index]["error_class"] = json!("rpc_reverted_invalid_or_timed_out"),
        }
    }
    rpc.verify_block_hash(height, hash).await?;
    let unavailable = rows.iter().filter(|r| r["status"] != "available").count();
    Ok(
        json!({"network_id":network,"address":address,"source":"finalized_rpc",
        "status":if unavailable == rows.len() {"unavailable"} else if unavailable > 0 || truncated {"partial"} else {"available"},
        "block_number":height,"block_hash":format!("{hash:#x}"),"assets":rows,"truncated":truncated,
        "unavailable_assets":unavailable,"observed_at_unix_ms":now_ms(),"persisted":false,
        "scope":"ETH plus assets discovered in stored wallet transfers; not undiscovered tokens",
        "observation_scope":"current_finalized_not_historical_window","unavailable_is_not_zero":true}),
    )
}

fn token_call(network: &str, asset: &Asset, wallet: Address) -> Result<(Address, Vec<u8>, bool)> {
    let prefix = format!("{network}/");
    let (standard, contract) = asset
        .asset_id
        .strip_prefix(&prefix)
        .context("wrong asset network")?
        .split_once(':')
        .context("invalid asset namespace")?;
    let contract = Address::from_str(contract).context("invalid token contract")?;
    let mut wallet_word = [0u8; 32];
    wallet_word[12..].copy_from_slice(wallet.as_slice());
    let mut bytes = match standard {
        "erc20" => vec![0x70, 0xa0, 0x82, 0x31],
        "erc721" => vec![0x63, 0x52, 0x21, 0x1e],
        "erc1155" => vec![0x00, 0xfd, 0xd5, 0x8e],
        _ => anyhow::bail!("unsupported token standard"),
    };
    if standard != "erc721" {
        bytes.extend_from_slice(&wallet_word);
    }
    if standard != "erc20" {
        ensure!(
            !asset.token_id.is_empty()
                && asset.token_id.len() <= 78
                && asset.token_id.bytes().all(|b| b.is_ascii_digit()),
            "invalid token id"
        );
        let id = U256::from_str_radix(&asset.token_id, 10)?;
        bytes.extend_from_slice(&id.to_be_bytes::<32>());
    }
    Ok((contract, bytes, standard == "erc721"))
}

fn decode_balance(bytes: &[u8], owner_of: bool, wallet: Address) -> Result<String> {
    ensure!(bytes.len() == 32, "balance response must be one ABI word");
    if owner_of {
        ensure!(bytes[..12].iter().all(|b| *b == 0), "invalid owner address");
        Ok(if &bytes[12..] == wallet.as_slice() {
            "1"
        } else {
            "0"
        }
        .to_string())
    } else {
        Ok(U256::from_be_slice(bytes).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructs_standard_specific_balance_calls() {
        let wallet = Address::repeat_byte(0x11);
        for (standard, length, selector) in [
            ("erc20", 36, "70a08231"),
            ("erc721", 36, "6352211e"),
            ("erc1155", 68, "00fdd58e"),
        ] {
            let asset = Asset {
                asset_id: format!("eip155:1/{standard}:0x{}", "22".repeat(20)),
                token_id: U256::MAX.to_string(),
                symbol: String::new(),
                decimals: None,
            };
            let (_, data, owner) = token_call("eip155:1", &asset, wallet).unwrap();
            assert_eq!(data.len(), length);
            assert_eq!(alloy::hex::encode(&data[..4]), selector);
            assert_eq!(owner, standard == "erc721");
            assert!(token_call("eip155:56", &asset, wallet).is_err());
        }
    }
    #[test]
    fn zero_is_a_valid_balance_but_missing_or_malformed_abi_is_not_zero() {
        let wallet = Address::repeat_byte(0x11);
        assert_eq!(decode_balance(&[0; 32], false, wallet).unwrap(), "0");
        assert_eq!(
            decode_balance(&[255; 32], false, wallet).unwrap(),
            U256::MAX.to_string()
        );
        assert!(decode_balance(&[], false, wallet).is_err());
        assert!(decode_balance(&[255; 32], true, wallet).is_err());
        let mut owner = [0; 32];
        owner[12..].copy_from_slice(wallet.as_slice());
        assert_eq!(decode_balance(&owner, true, wallet).unwrap(), "1");
        assert_eq!(decode_balance(&[0; 32], true, wallet).unwrap(), "0");
        assert_eq!(
            unavailable("eip155:1", "wallet", "rpc_unavailable")["status"],
            "unavailable"
        );
    }
}
