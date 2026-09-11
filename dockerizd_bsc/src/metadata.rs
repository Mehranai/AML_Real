use crate::{
    BSC_NETWORK_ID, config::AppConfig, db::warehouse::Warehouse, domain::normalize_evm_address,
    investigation::now_ms,
};
use anyhow::{Context, Result, bail, ensure};
// These lints originate in the upstream macro, not the metadata implementation.
#[allow(clippy::manual_div_ceil, clippy::assign_op_pattern)]
mod integer {
    uint::construct_uint! { pub struct U256(4); }
}
use integer::U256;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct TokenRpc {
    client: reqwest::Client,
    config: AppConfig,
}
impl TokenRpc {
    pub fn new(config: AppConfig) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(config.rpc_timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            config,
        })
    }
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut response = self
            .client
            .post(self.config.rpc_endpoint.as_url().clone())
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("RPC transport unavailable"))?;
        ensure!(response.status().is_success(), "RPC HTTP failure");
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("RPC body unavailable"))?
        {
            ensure!(
                body.len() + chunk.len() <= 2 * 1024 * 1024,
                "RPC response exceeds limit"
            );
            body.extend_from_slice(&chunk);
        }
        let v: Value = serde_json::from_slice(&body)?;
        ensure!(
            v["jsonrpc"] == "2.0" && v["id"] == 1,
            "RPC response identity mismatch"
        );
        ensure!(
            v["error"].is_null() && !v["result"].is_null(),
            "RPC method {method} unavailable or reverted"
        );
        Ok(v["result"].clone())
    }
    pub async fn finalized(&self) -> Result<Value> {
        let chain = self.call("eth_chainId", json!([])).await?;
        ensure!(
            quantity(chain.as_str().context("missing chain ID")?)? == U256::from(56),
            "RPC must be BSC mainnet (56)"
        );
        let block = self
            .call("eth_getBlockByNumber", json!(["finalized", false]))
            .await?;
        ensure!(
            block["hash"].as_str().is_some() && block["number"].as_str().is_some(),
            "missing finalized identity"
        );
        let hash = block["hash"].as_str().unwrap();
        ensure!(
            hash.len() == 66
                && hash.starts_with("0x")
                && hash[2..].bytes().all(|c| c.is_ascii_hexdigit()),
            "invalid finalized hash"
        );
        block_height(&block)?;
        Ok(block)
    }
    async fn eth_call(&self, token: &str, data: &str, block: &Value) -> Result<String> {
        self.call(
            "eth_call",
            json!([{"to":token,"data":data},block["number"]]),
        )
        .await?
        .as_str()
        .map(str::to_owned)
        .context("non-string eth_call")
    }
    async fn verify(&self, block: &Value) -> Result<()> {
        let after = self
            .call("eth_getBlockByNumber", json!([block["number"], false]))
            .await?;
        ensure!(
            after["hash"] == block["hash"],
            "finalized block changed during RPC reads; retry"
        );
        Ok(())
    }
}

fn block_height(block: &Value) -> Result<u64> {
    let height = quantity(block["number"].as_str().context("missing block number")?)?;
    ensure!(
        height <= U256::from(u64::MAX),
        "block height exceeds UInt64"
    );
    Ok(height.low_u64())
}

fn quantity(raw: &str) -> Result<U256> {
    let hex = raw.strip_prefix("0x").context("missing hex prefix")?;
    ensure!(
        !hex.is_empty() && hex.len() <= 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid UInt256"
    );
    U256::from_str_radix(hex, 16).context("invalid UInt256")
}
fn abi_number(raw: &str) -> Result<U256> {
    ensure!(raw.len() == 66, "ABI number must contain one full word");
    quantity(raw)
}
fn word(value: U256) -> String {
    format!("{value:064x}")
}
fn abi_text(raw: &str) -> Result<String> {
    let hex = raw.strip_prefix("0x").context("ABI prefix")?;
    ensure!(
        hex.len() % 2 == 0 && hex.len() <= 8192 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "ABI string size or encoding"
    );
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()?;
    let text = if bytes.len() == 32 {
        bytes.split(|b| *b == 0).next().unwrap_or(&[])
    } else {
        ensure!(
            bytes.len() >= 64 && bytes[..31].iter().all(|b| *b == 0) && bytes[31] == 32,
            "ABI offset"
        );
        let size = U256::from_big_endian(&bytes[32..64]);
        ensure!(size <= U256::from(512), "metadata text too long");
        let size = size.as_usize();
        ensure!(64 + size <= bytes.len(), "truncated ABI string");
        &bytes[64..64 + size]
    };
    let value = std::str::from_utf8(text)?.to_owned();
    ensure!(
        !value.chars().any(char::is_control),
        "control characters in metadata"
    );
    Ok(value)
}

pub async fn worker_once(db: &Warehouse, rpc: &TokenRpc) -> Result<usize> {
    let now = now_ms().to_string();
    let pending=db.rows("SELECT token_address,standard_hint FROM token_metadata_discoveries_canonical
        WHERE token_address NOT IN (SELECT token_address FROM token_metadata_current WHERE metadata_status='complete')
        AND token_address NOT IN (SELECT token_address FROM token_metadata_jobs_current
            WHERE next_attempt_at_unix_ms>{now:UInt64} OR attempt_count>=10)
        ORDER BY discovered_block,token_address LIMIT 25",&[("now",&now)]).await?;
    if pending.is_empty() {
        return Ok(0);
    }
    let block = rpc.finalized().await?;
    let height = block_height(&block)?;
    for item in &pending {
        let token = item["token_address"].as_str().context("token address")?;
        let standard = item["standard_hint"].as_str().unwrap_or("unknown");
        let attempts=db.rows("SELECT attempt_count FROM token_metadata_jobs_current WHERE token_address={token:String}",&[("token",token)]).await?;
        let attempt = attempts
            .first()
            .and_then(|v| v["attempt_count"].as_u64())
            .unwrap_or(0)
            + 1;
        let name = match rpc.eth_call(token, "0x06fdde03", &block).await {
            Ok(v) => abi_text(&v).ok(),
            Err(_) => None,
        };
        let symbol = match rpc.eth_call(token, "0x95d89b41", &block).await {
            Ok(v) => abi_text(&v).ok(),
            Err(_) => None,
        };
        let decimals = match rpc.eth_call(token, "0x313ce567", &block).await {
            Ok(v) => abi_number(&v)
                .ok()
                .filter(|n| *n <= U256::from(255))
                .map(|n| n.low_u32() as u8),
            Err(_) => None,
        };
        let complete = if standard.eq_ignore_ascii_case("erc20") {
            decimals.is_some() && symbol.is_some()
        } else {
            name.is_some() || symbol.is_some()
        };
        rpc.verify(&block).await?;
        db.insert("token_metadata",&[json!({"network_id":BSC_NETWORK_ID,"token_address":token,"token_standard":standard,
            "name":name.unwrap_or_default(),"symbol":symbol.unwrap_or_default(),"decimals":decimals,
            "metadata_status":if complete {"complete"}else{"partial"},"metadata_source":"rpc","is_verified":0,
            "observed_block":height,"created_at_unix_ms":now_ms(),"source_reference":block["hash"],"reviewed_by":""})]).await?;
        db.insert("token_metadata_jobs",&[json!({"network_id":BSC_NETWORK_ID,"token_address":token,
            "status":if complete {"complete"}else{"retry"},"attempt_count":attempt,
            "last_error_class":if complete {""}else{"metadata_unavailable"},
            "next_attempt_at_unix_ms":now_ms()+60_000*(1u64<<attempt.min(10)),"updated_at_unix_ms":now_ms()})]).await?;
    }
    Ok(pending.len())
}

pub async fn import_metadata(db: &Warehouse, mut rows: Vec<Value>, dry_run: bool) -> Result<usize> {
    ensure!(
        !rows.is_empty() && rows.len() <= 10000,
        "metadata import size"
    );
    for row in &mut rows {
        ensure!(
            row["network_id"] == BSC_NETWORK_ID,
            "metadata network mismatch"
        );
        row["token_address"] = json!(normalize_evm_address(
            row["token_address"]
                .as_str()
                .context("token_address required")?
        )?);
        for field in [
            "reviewed_by",
            "source_reference",
            "name",
            "symbol",
            "token_standard",
        ] {
            ensure!(
                row[field].as_str().is_some_and(|v| !v.trim().is_empty()
                    && v.len() <= 512
                    && !v.chars().any(char::is_control)),
                "invalid metadata {field}"
            );
        }
        ensure!(
            matches!(
                row["token_standard"].as_str(),
                Some("erc20" | "erc721" | "erc1155")
            ),
            "invalid token standard"
        );
        ensure!(
            row["decimals"].is_null() || row["decimals"].as_u64().is_some_and(|n| n <= 255),
            "invalid decimals"
        );
        ensure!(
            row["token_standard"] != "erc20" || row["decimals"].as_u64().is_some(),
            "ERC20 decimals required"
        );
        let allowed = [
            "network_id",
            "token_address",
            "name",
            "symbol",
            "token_standard",
            "decimals",
            "reviewed_by",
            "source_reference",
        ];
        ensure!(
            row.as_object()
                .context("metadata object")?
                .keys()
                .all(|k| allowed.contains(&k.as_str())),
            "unknown metadata field"
        );
        row["metadata_status"] = json!("complete");
        row["metadata_source"] = json!("manual");
        row["is_verified"] = json!(1);
        row["observed_block"] = json!(0);
        row["created_at_unix_ms"] = json!(now_ms());
    }
    if !dry_run {
        db.insert("token_metadata", &rows).await?;
    }
    Ok(rows.len())
}

pub async fn holdings(db: &Warehouse, rpc: &TokenRpc, address: &str) -> Result<Value> {
    let block = rpc.finalized().await?;
    let mut assets=db.rows("SELECT DISTINCT asset_id,token_id FROM address_relationships_canonical
        WHERE from_address={address:String} OR to_address={address:String} ORDER BY asset_id,token_id LIMIT 101",&[("address",address)]).await?;
    assets.retain(|v| v["asset_id"] != "eip155:56/native:bnb");
    let truncated = assets.len() > 100;
    assets.truncate(100);
    assets.insert(0, json!({"asset_id":"eip155:56/native:bnb","token_id":""}));
    let mut rows = Vec::new();
    for asset in assets {
        let id = asset["asset_id"].as_str().context("asset id")?;
        let result: Result<String> = async {
            if id.ends_with("/native:bnb") {
                return Ok(quantity(
                    rpc.call("eth_getBalance", json!([address, block["number"]]))
                        .await?
                        .as_str()
                        .context("balance")?,
                )?
                .to_string());
            }
            let (_, part) = id.split_once('/').context("asset namespace")?;
            let (standard, rest) = part.split_once(':').context("asset standard")?;
            let token = normalize_evm_address(rest.split('/').next().context("token")?)?;
            let wallet = format!("{:0>64}", address.trim_start_matches("0x"));
            let data = match standard {
                "erc20" => format!("0x70a08231{wallet}"),
                "erc721" | "erc1155" => {
                    let id =
                        U256::from_str_radix(asset["token_id"].as_str().context("token id")?, 10)?;
                    if standard == "erc721" {
                        format!("0x6352211e{}", word(id))
                    } else {
                        format!("0x00fdd58e{wallet}{}", word(id))
                    }
                }
                _ => bail!("unknown token standard"),
            };
            let raw = rpc.eth_call(&token, &data, &block).await?;
            let value = abi_number(&raw)?;
            if standard == "erc721" {
                ensure!(value.bits() <= 160, "invalid ABI address");
                let owner = format!("0x{:040x}", value);
                Ok(if owner.eq_ignore_ascii_case(address) {
                    "1"
                } else {
                    "0"
                }
                .to_owned())
            } else {
                Ok(value.to_string())
            }
        }
        .await;
        rows.push(match result {Ok(amount)=>json!({"asset_id":id,"token_id":asset["token_id"],"amount":amount,"status":"available"}),
            Err(_)=>json!({"asset_id":id,"token_id":asset["token_id"],"amount":null,"status":"unavailable"})});
    }
    rpc.verify(&block).await?;
    Ok(
        json!({"network_id":BSC_NETWORK_ID,"address":address,"source":"finalized_rpc","block_number":block_height(&block)?,
        "block_hash":block["hash"],"assets":rows,"truncated":truncated,"observed_at_unix_ms":now_ms(),
        "scope":"BNB plus assets discovered in stored wallet transfers; not all undiscovered tokens",
        "persisted":false,"unavailable_is_not_zero":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn amounts_remain_full_width() {
        assert_eq!(
            quantity(&format!("0x{}", "f".repeat(64)))
                .unwrap()
                .to_string(),
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
        assert!(quantity("0x").is_err());
        assert!(quantity(&format!("0x1{}", "0".repeat(64))).is_err());
    }
    #[test]
    fn abi_text_checks_offsets_and_lengths() {
        assert_eq!(abi_text(&format!("0x{:0<64}", "425343")).unwrap(), "BSC");
        assert!(abi_text("0xzz").is_err());
        assert!(abi_text(&format!("0x{}", "f".repeat(128))).is_err());
    }
}
