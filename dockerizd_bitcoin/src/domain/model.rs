use anyhow::{Context, Result, ensure};
use bitcoin::{Address, Amount, Denomination, Network, ScriptBuf, address::NetworkUnchecked};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, str::FromStr};

pub fn address(raw: &str) -> Result<String> {
    Ok(Address::<NetworkUnchecked>::from_str(raw)
        .context("invalid Bitcoin address")?
        .require_network(Network::Bitcoin)
        .context("Bitcoin mainnet address required")?
        .to_string())
}
pub fn hash(raw: &str) -> Result<String> {
    aml_warehouse::values::normalize_hash(aml_warehouse::Network::Bitcoin, raw)
}
pub fn satoshis(value: &Value) -> Result<u64> {
    let decimal = if let Some(v) = value.as_str() {
        v.to_owned()
    } else {
        ensure!(value.is_number(), "BTC value must be a decimal number");
        value.to_string()
    };
    // Core returns decimal JSON numbers. arbitrary_precision keeps every digit intact.
    let amount = Amount::from_str_in(&decimal, Denomination::Bitcoin)
        .context("invalid BTC value")?
        .to_sat();
    ensure!(
        amount <= 2_100_000_000_000_000,
        "BTC value exceeds total supply"
    );
    Ok(amount)
}
pub fn decimal(sats: u64) -> String {
    // Native precision is part of the common contract; formatting stays exact.
    aml_warehouse::RawAmount::parse(&sats.to_string())
        .expect("u64 fits UInt256")
        .decimal(aml_warehouse::Network::Bitcoin.native_decimals())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptOutput {
    pub address: String,
    pub script_hex: String,
    pub script_type: String,
    pub value_sats: u64,
}
impl ScriptOutput {
    pub fn from_rpc(value: &Value) -> Result<Self> {
        let script = value.get("scriptPubKey").context("missing scriptPubKey")?;
        let hex = script["hex"].as_str().context("missing script hex")?;
        let bytes = ScriptBuf::from_hex(hex).context("invalid script hex")?;
        // Derive addresses from the actual script, including Taproot. Do not assign bare
        // multisig or P2PK outputs to any single participant reported by old RPC versions.
        let address = Address::from_script(&bytes, Network::Bitcoin)
            .map(|v| v.to_string())
            .unwrap_or_default();
        Ok(Self {
            address,
            script_hex: hex.to_ascii_lowercase(),
            script_type: script["type"].as_str().unwrap_or("unknown").into(),
            value_sats: satoshis(&value["value"])?,
        })
    }
    pub fn subject(&self) -> String {
        if self.address.is_empty() {
            format!("script:{:x}", Sha256::digest(self.script_hex.as_bytes()))
        } else {
            self.address.clone()
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub commit_id: String,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: u64,
    pub txid: String,
    pub tx_index: u32,
    pub coinbase: u8,
    pub input_count: u32,
    pub output_count: u32,
    pub output_sats: u64,
    pub fee_sats: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output {
    pub commit_id: String,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: u64,
    pub txid: String,
    pub tx_index: u32,
    pub output_index: u32,
    pub coinbase: u8,
    #[serde(flatten)]
    pub output: ScriptOutput,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub commit_id: String,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: u64,
    pub txid: String,
    pub tx_index: u32,
    pub input_index: u32,
    pub prev_txid: String,
    pub prev_vout: u32,
    #[serde(flatten)]
    pub output: ScriptOutput,
}
#[derive(Default)]
pub struct Evidence {
    pub transactions: Vec<Transaction>,
    pub outputs: Vec<Output>,
    pub inputs: Vec<Input>,
}
pub fn outpoint(txid: &str, n: u32) -> String {
    format!("{txid}:{n}")
}

pub fn extract(
    block: &Value,
    commit_id: &str,
    resolved: &HashMap<String, ScriptOutput>,
) -> Result<Evidence> {
    let height = block["height"].as_u64().context("missing block height")?;
    let block_hash = hash(block["hash"].as_str().context("missing block hash")?)?;
    let time = block["time"].as_u64().context("missing block timestamp")?;
    let transactions = block["tx"]
        .as_array()
        .context("getblock must return decoded transactions")?;
    ensure!(!transactions.is_empty(), "empty Bitcoin block");
    let mut evidence = Evidence::default();
    let mut available = resolved.clone();
    let mut spent = std::collections::HashSet::new();
    for (index, tx) in transactions.iter().enumerate() {
        let txid = hash(tx["txid"].as_str().context("missing txid")?)?;
        let vin = tx["vin"].as_array().context("missing inputs")?;
        let vout = tx["vout"].as_array().context("missing outputs")?;
        ensure!(!vin.is_empty() && !vout.is_empty(), "empty transaction");
        let coinbase = u8::from(index == 0 && vin.len() == 1 && vin[0].get("coinbase").is_some());
        ensure!((index == 0) == (coinbase == 1), "invalid coinbase position");
        let mut input_total = 0u64;
        if coinbase == 0 {
            for (i, input) in vin.iter().enumerate() {
                let previous = hash(input["txid"].as_str().context("missing prevout txid")?)?;
                let n = u32::try_from(input["vout"].as_u64().context("missing prevout index")?)?;
                let key = outpoint(&previous, n);
                ensure!(
                    spent.insert(key.clone()),
                    "duplicate input outpoint in block"
                );
                let output = if let Some(found) = available.get(&key) {
                    found.clone()
                } else {
                    ScriptOutput::from_rpc(
                        input
                            .get("prevout")
                            .context("unresolved input; need prevout data or txindex=1")?,
                    )?
                };
                input_total = input_total
                    .checked_add(output.value_sats)
                    .context("input amount overflow")?;
                evidence.inputs.push(Input {
                    commit_id: commit_id.into(),
                    block_height: height,
                    block_hash: block_hash.clone(),
                    block_time: time,
                    txid: txid.clone(),
                    tx_index: index as u32,
                    input_index: i as u32,
                    prev_txid: previous,
                    prev_vout: n,
                    output,
                });
            }
        }
        let mut output_total = 0u64;
        for (i, output) in vout.iter().enumerate() {
            ensure!(
                output["n"].as_u64() == Some(i as u64),
                "non-contiguous output indexes"
            );
            let output = ScriptOutput::from_rpc(output)?;
            output_total = output_total
                .checked_add(output.value_sats)
                .context("output amount overflow")?;
            available.insert(outpoint(&txid, i as u32), output.clone());
            evidence.outputs.push(Output {
                commit_id: commit_id.into(),
                block_height: height,
                block_hash: block_hash.clone(),
                block_time: time,
                txid: txid.clone(),
                tx_index: index as u32,
                output_index: i as u32,
                coinbase,
                output,
            });
        }
        let fee = if coinbase == 1 {
            0
        } else {
            input_total
                .checked_sub(output_total)
                .context("outputs exceed resolved inputs")?
        };
        evidence.transactions.push(Transaction {
            commit_id: commit_id.into(),
            block_height: height,
            block_hash: block_hash.clone(),
            block_time: time,
            txid,
            tx_index: index as u32,
            coinbase,
            input_count: vin.len() as u32,
            output_count: vout.len() as u32,
            output_sats: output_total,
            fee_sats: fee,
        });
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    pub fn block() -> Value {
        let script = "76a914000000000000000000000000000000000000000088ac";
        json!({"height":1,"hash":"a".repeat(64),"previousblockhash":"b".repeat(64),"time":123,
          "tx":[{"txid":"1".repeat(64),"vin":[{"coinbase":"00"}],"vout":[{"n":0,"value":50,"scriptPubKey":{"hex":script,"type":"pubkeyhash"}}]},
            {"txid":"2".repeat(64),"vin":[{"txid":"3".repeat(64),"vout":0,"prevout":{"value":1,"scriptPubKey":{"hex":script,"type":"pubkeyhash"}}}],
             "vout":[{"n":0,"value":0.99999999,"scriptPubKey":{"hex":script,"type":"pubkeyhash"}}]}]})
    }
    #[test]
    fn money_is_exact_and_invalid_values_fail() {
        assert_eq!(
            satoshis(&serde_json::from_str("0.00000001").unwrap()).unwrap(),
            1
        );
        assert_eq!(
            satoshis(&json!("21000000.00000000")).unwrap(),
            2_100_000_000_000_000
        );
        for value in ["0.000000001", "-1", "21000001", "NaN"] {
            assert!(satoshis(&json!(value)).is_err());
        }
        assert_eq!(decimal(100000001), "1.00000001");
    }
    #[test]
    fn validates_checksum_network_and_canonical_case() {
        let legacy = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT";
        assert_eq!(address(legacy).unwrap(), legacy);
        assert!(address("1BoatSLRHtKNngkdXEeobR76b53LETtpyU").is_err());
        assert!(address("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn").is_err());
        let taproot = "bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr";
        assert_eq!(address(&taproot.to_uppercase()).unwrap(), taproot);
    }
    #[test]
    fn exact_fees_coinbase_and_missing_inputs() {
        let mut block = block();
        let evidence = extract(&block, "commit", &HashMap::new()).unwrap();
        assert_eq!(evidence.inputs.len(), 1);
        assert_eq!(evidence.transactions[1].fee_sats, 1);
        assert_eq!(evidence.outputs[0].coinbase, 1);
        block["tx"][1]["vin"][0]
            .as_object_mut()
            .unwrap()
            .remove("prevout");
        assert!(extract(&block, "commit", &HashMap::new()).is_err());
    }
    #[test]
    fn same_block_outputs_are_resolved_without_rpc_and_overspending_fails() {
        let mut block = block();
        block["tx"][1]["vin"][0] = json!({"txid":"1".repeat(64),"vout":0});
        assert!(extract(&block, "commit", &HashMap::new()).is_ok());
        block["tx"][1]["vout"][0]["value"] = json!(51);
        assert!(extract(&block, "commit", &HashMap::new()).is_err());
    }
}
