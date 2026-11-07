//! Source checks used before the live ingestor accepts financial fields.
//!
//! SHA256(raw_data_hex) authenticates only the supplied protobuf bytes against
//! txID. It does not verify signatures, block inclusion, consensus or an RPC
//! receipt. Receipts are linked by id/height/time and retained as RPC evidence.
//! Field numbers: tronprotocol/protocol/core/Tron.proto and core/contract/*.
use std::collections::BTreeMap;

use aml_warehouse::{Network, publication::Block};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::utils::tron_address::{base58_to_hex, normalize_tron_address};

const MAX_RAW_BYTES: usize = 4 * 1024 * 1024;
const MAX_FIELDS: usize = 100_000;

#[derive(Debug)]
enum Wire<'a> {
    Integer(u64),
    Bytes(&'a [u8]),
    Fixed,
}
struct Message<'a>(Vec<(u32, Wire<'a>)>);

fn varint(input: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *input.get(*cursor).context("truncated protobuf varint")?;
        *cursor += 1;
        ensure!(shift < 63 || byte <= 1, "protobuf varint overflow");
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
    }
    anyhow::bail!("protobuf varint overflow")
}
impl<'a> Message<'a> {
    fn parse(input: &'a [u8]) -> Result<Self> {
        ensure!(
            input.len() <= MAX_RAW_BYTES,
            "protobuf source exceeds limit"
        );
        let mut fields = Vec::new();
        let mut cursor = 0;
        while cursor < input.len() {
            ensure!(fields.len() < MAX_FIELDS, "protobuf field limit exceeded");
            let tag = varint(input, &mut cursor)?;
            let number = u32::try_from(tag >> 3)?;
            ensure!(
                (1..(1 << 29)).contains(&number),
                "invalid protobuf field number"
            );
            let wire = match tag & 7 {
                0 => Wire::Integer(varint(input, &mut cursor)?),
                kind @ (1 | 2 | 5) => {
                    let len = match kind {
                        1 => 8,
                        5 => 4,
                        _ => usize::try_from(varint(input, &mut cursor)?)?,
                    };
                    let end = cursor
                        .checked_add(len)
                        .context("protobuf length overflow")?;
                    let bytes = input.get(cursor..end).context("truncated protobuf field")?;
                    cursor = end;
                    if kind == 2 {
                        Wire::Bytes(bytes)
                    } else {
                        Wire::Fixed
                    }
                }
                _ => anyhow::bail!("unsupported protobuf wire type"),
            };
            fields.push((number, wire));
        }
        Ok(Self(fields))
    }
    fn one(&self, number: u32) -> Result<Option<&Wire<'a>>> {
        let mut found = self.0.iter().filter(|(n, _)| *n == number);
        let value = found.next().map(|(_, v)| v);
        ensure!(
            found.next().is_none(),
            "ambiguous duplicate protobuf scalar {number}"
        );
        Ok(value)
    }
    fn integer(&self, number: u32) -> Result<u64> {
        match self.one(number)? {
            None => Ok(0), // proto3 scalar default, not a missing RPC observation.
            Some(Wire::Integer(value)) => Ok(*value),
            _ => anyhow::bail!("wrong protobuf integer wire type"),
        }
    }
    fn bytes(&self, number: u32) -> Result<&'a [u8]> {
        match self.one(number)? {
            None => Ok(&[]),
            Some(Wire::Bytes(value)) => Ok(value),
            _ => anyhow::bail!("wrong protobuf bytes wire type"),
        }
    }
    fn repeated_bytes(&self, number: u32) -> Result<Vec<&'a [u8]>> {
        self.0
            .iter()
            .filter(|(n, _)| *n == number)
            .map(|(_, wire)| match wire {
                Wire::Bytes(value) => Ok(*value),
                _ => anyhow::bail!("wrong protobuf repeated-message wire type"),
            })
            .collect()
    }
}

#[derive(Debug)]
struct ContractEvidence {
    type_id: u32,
    type_url: String,
    value_hex: String,
    fields_verified: bool,
}

/// A value can be constructed only after its original bytes and receipt have
/// passed source checks. Unknown contract payloads retain an explicit false flag.
#[derive(Debug)]
pub struct VerifiedTransaction {
    tx: Value,
    receipt: Option<Value>,
    contracts: Vec<ContractEvidence>,
    tx_hash: String,
    block_number: u64,
    block_timestamp: u64,
}
impl VerifiedTransaction {
    pub fn verify(
        tx: &Value,
        receipt: &Value,
        block_number: u64,
        block_timestamp: u64,
    ) -> Result<Self> {
        ensure!(
            block_number > 0,
            "genesis allocations have no runtime receipt"
        );
        let mut verified = Self::transaction(tx, block_number, block_timestamp)?;
        ensure!(receipt.is_object(), "transaction receipt is not available");
        let id = receipt["id"]
            .as_str()
            .context("transaction receipt is not available")?;
        ensure!(
            canonical_hash(id)? == verified.tx_hash,
            "receipt transaction id mismatch"
        );
        ensure!(
            receipt["blockNumber"].as_u64() == Some(block_number),
            "receipt block number mismatch"
        );
        if let Some(time) = receipt.get("blockTimeStamp") {
            ensure!(
                time.as_u64() == Some(block_timestamp),
                "receipt block timestamp mismatch"
            );
        }
        if let Some(resource) = receipt.get("receipt") {
            ensure!(resource.is_object(), "invalid receipt resource object");
        }
        optional_string(receipt, "result")?;
        optional_string(&receipt["receipt"], "result")?;
        // Validate source types before any legacy or common financial writer runs.
        optional_u64(receipt, "fee")?;
        for key in [
            "energy_usage",
            "energy_usage_total",
            "origin_energy_usage",
            "energy_fee",
            "net_usage",
            "net_fee",
            "energy_penalty_total",
        ] {
            optional_u64(&receipt["receipt"], key)?;
        }
        for log in array(receipt, "log")? {
            address(log.get("address").context("log emitter missing")?)?;
            for topic in array(log, "topics")? {
                ensure!(hex_bytes(topic)?.len() == 64, "log topic must be 32 bytes");
            }
            hex_bytes(log.get("data").unwrap_or(&json!("")))?;
        }
        for call in array(receipt, "internal_transactions")? {
            ensure!(call.is_object(), "invalid internal transaction object");
            if let Some(hash) = call.get("hash") {
                canonical_hash(hash.as_str().context("invalid internal hash")?)?;
            }
            for key in ["caller_address", "transferTo_address"] {
                if let Some(value) = call.get(key) {
                    address(value)?;
                }
            }
            if let Some(value) = call.get("rejected") {
                ensure!(value.is_boolean(), "invalid internal rejected flag");
            }
            if let Some(value) = call.get("note") {
                hex_bytes(value)?;
            }
            for value in array(call, "callValueInfo")? {
                ensure!(value.is_object(), "invalid internal call value object");
                optional_u64(value, "callValue")?;
                if let Some(token) = value.get("tokenId") {
                    ensure!(token.is_string(), "invalid internal token id");
                }
            }
        }
        verified.receipt = Some(receipt.clone());
        Ok(verified)
    }
    /// Genesis source transactions are retained without invented runtime results.
    pub fn genesis(tx: &Value, block_timestamp: u64) -> Result<Self> {
        Self::transaction(tx, 0, block_timestamp)
    }
    fn transaction(tx: &Value, block_number: u64, block_timestamp: u64) -> Result<Self> {
        let tx_hash = canonical_hash(tx["txID"].as_str().context("missing txID")?)?;
        let raw_hex = tx["raw_data_hex"]
            .as_str()
            .context("missing original raw_data_hex")?;
        ensure!(
            raw_hex.len() <= MAX_RAW_BYTES * 2,
            "raw_data_hex exceeds source limit"
        );
        let raw = hex::decode(raw_hex).context("invalid raw_data_hex")?;
        ensure!(
            format!("{:x}", Sha256::digest(&raw)) == tx_hash,
            "raw_data_hex does not match txID"
        );
        let proto = Message::parse(&raw)?;
        let json_raw = &tx["raw_data"];
        ensure!(json_raw.is_object(), "missing raw_data JSON");
        for result in array(tx, "ret")? {
            ensure!(result.is_object(), "invalid transaction result object");
            optional_string(result, "ret")?;
            optional_string(result, "contractRet")?;
        }
        for (number, key) in [
            (1, "ref_block_bytes"),
            (4, "ref_block_hash"),
            (10, "data"),
            (12, "scripts"),
        ] {
            match_bytes(&proto, number, json_raw, key, false)?;
        }
        for (number, key) in [
            (3, "ref_block_num"),
            (8, "expiration"),
            (14, "timestamp"),
            (18, "fee_limit"),
        ] {
            match_integer(&proto, number, json_raw, key)?;
        }
        let proto_contracts = proto.repeated_bytes(11)?;
        let json_contracts = array(json_raw, "contract")?;
        ensure!(
            proto_contracts.len() == json_contracts.len(),
            "protobuf/JSON contract count mismatch"
        );
        ensure!(
            !proto_contracts.is_empty(),
            "transaction has no protobuf contract"
        );
        let contracts = proto_contracts
            .into_iter()
            .zip(json_contracts)
            .map(|(bytes, contract)| verify_contract(bytes, contract, block_number == 0))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            tx: tx.clone(),
            receipt: None,
            contracts,
            tx_hash,
            block_number,
            block_timestamp,
        })
    }
    pub fn tx_hash(&self) -> &str {
        &self.tx_hash
    }
    pub fn raw_transaction(&self) -> &Value {
        &self.tx
    }
    pub fn receipt(&self) -> Option<&Value> {
        self.receipt.as_ref()
    }
    pub fn parameter_verified(&self, index: usize) -> bool {
        self.contracts.get(index).is_some_and(|c| c.fields_verified)
    }
    pub fn execution_succeeded(&self) -> bool {
        self.receipt
            .as_ref()
            .is_some_and(|receipt| transaction_succeeded(&self.tx, receipt))
    }

    /// Structured source facts; block publication remains the shared core's job.
    /// Values omitted by the RPC remain NULL. No successful-flow edges are made here.
    pub fn facts(
        &self,
        block: &Block,
        transaction_index: u32,
        observed_at: u64,
    ) -> Result<BTreeMap<String, Vec<Value>>> {
        ensure!(
            block.network_id == Network::Tron.id()
                && block.block_number == self.block_number
                && block.block_timestamp_unix_ms == self.block_timestamp,
            "TRON evidence block context mismatch"
        );
        ensure!(
            transaction_index < block.transaction_count,
            "TRON evidence transaction index outside block"
        );
        let mut output: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let make = |extra: Value| -> Value {
            let mut row = json!({"network_id":block.network_id,"block_number":block.block_number,"block_hash":block.block_hash,"block_timestamp_unix_ms":block.block_timestamp_unix_ms,"commit_id":block.commit_id,"revision":block.revision,"tx_hash":self.tx_hash,"transaction_index":transaction_index,"created_at_unix_ms":observed_at});
            row.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            row
        };
        let contracts = array(&self.tx["raw_data"], "contract")?.iter().zip(&self.contracts).enumerate().map(|(index, (original, proof))| {
            Ok(make(json!({"contract_index":u32::try_from(index)?,"contract_type":original["type"],"payload_json":serde_json::to_string(original)?,"proto_contract_type":proof.type_id,"type_url":proof.type_url,"parameter_value_hex":proof.value_hex,"raw_data_hash_verified":1,"parameter_fields_verified":u8::from(proof.fields_verified)})))
        }).collect::<Result<Vec<_>>>()?;
        output.insert("aml_tron_contracts".into(), contracts);
        output.insert("aml_raw_transactions".into(), vec![make(json!({"source_id":block.source_id,"payload_json":serde_json::to_string(&self.tx)?,"receipt_json":self.receipt.as_ref().map(serde_json::to_string).transpose()?,"evidence_refs":[format!("tx:{}", self.tx_hash),format!("block:{}",block.block_hash)]}))]);
        let mut logs = Vec::new();
        let mut internals = Vec::new();
        let mut resources = Vec::new();
        if let Some(receipt) = &self.receipt {
            for (index, original) in array(receipt, "log")?.iter().enumerate() {
                logs.push(make(json!({"log_index":u32::try_from(index)?,"address":address(&original["address"])?,"topics":array(original,"topics")?.iter().map(hex_bytes).collect::<Result<Vec<_>>>()?,"data_hex":hex_bytes(original.get("data").unwrap_or(&json!("")))?,"raw_evidence_json":serde_json::to_string(original)?})));
            }
            for (index, original) in array(receipt, "internal_transactions")?.iter().enumerate() {
                internals.push(make(json!({"internal_index":u32::try_from(index)?,"internal_hash":original.get("hash").map(|v| canonical_hash(v.as_str().context("invalid internal hash")?)).transpose()?,"caller_address":original.get("caller_address").map(address).transpose()?,"transfer_to_address":original.get("transferTo_address").map(address).transpose()?,"rejected":original.get("rejected").map(|v| u8::from(v.as_bool().unwrap())),"note_hex":original.get("note").map(hex_bytes).transpose()?,"call_value_info_json":serde_json::to_string(&array(original,"callValueInfo")?)?,"raw_evidence_json":serde_json::to_string(original)?})));
            }
            let mut resource = make(
                json!({"fee_raw":optional_u64(receipt,"fee")?.map(|n| n.to_string()),"receipt_result":optional_string(&receipt["receipt"],"result")?,"transaction_result":optional_string(receipt,"result")?,"raw_evidence_json":serde_json::to_string(receipt)?}),
            );
            for key in [
                "energy_usage",
                "energy_usage_total",
                "origin_energy_usage",
                "energy_fee",
                "net_usage",
                "net_fee",
                "energy_penalty_total",
            ] {
                resource[key] = json!(optional_u64(&receipt["receipt"], key)?);
            }
            resources.push(resource);
        }
        output.insert("aml_tron_logs".into(), logs);
        output.insert("aml_tron_internals".into(), internals);
        output.insert("aml_tron_resources".into(), resources);
        let contract = aml_warehouse::current_contract()?;
        for (table, rows) in &output {
            for row in rows {
                aml_warehouse::semantics::validate_row(&contract, table, row)?;
            }
        }
        Ok(output)
    }
}

pub(super) fn transaction_succeeded(tx: &Value, receipt: &Value) -> bool {
    // Proto3 omits SUCESS=0 in TransactionInfo; non-VM transfers may also
    // omit receipt.result. An explicit failure anywhere must win.
    let outer_ok = receipt
        .get("result")
        .is_none_or(|v| matches!(v.as_str(), Some("SUCESS" | "SUCCESS")));
    let vm_ok = receipt
        .get("receipt")
        .and_then(|v| v.get("result"))
        .is_none_or(|v| v.as_str() == Some("SUCCESS"));
    let tx_ok = array(tx, "ret").is_ok_and(|results| {
        results.iter().all(|result| {
            result
                .get("ret")
                .is_none_or(|v| matches!(v.as_str(), Some("SUCESS" | "SUCCESS")))
                && result
                    .get("contractRet")
                    .is_none_or(|v| matches!(v.as_str(), Some("SUCCESS" | "DEFAULT")))
        })
    });
    outer_ok && vm_ok && tx_ok
}

fn canonical_hash(value: &str) -> Result<String> {
    aml_warehouse::values::normalize_hash(Network::Tron, value)
}
fn hex_bytes(value: &Value) -> Result<String> {
    let text = value.as_str().context("hex bytes must be a string")?;
    ensure!(
        text.len() % 2 == 0 && text.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid source hex bytes"
    );
    Ok(text.to_ascii_lowercase())
}
fn address(value: &Value) -> Result<String> {
    let text = value.as_str().context("address must be a string")?;
    // Source addresses are exactly 21-byte TRON, 20-byte log emitters, or
    // Base58Check. Do not accept the permissive ABI-tail slicing helper here.
    ensure!(
        text.starts_with('T')
            || text.len() == 40
            || (text.len() == 42
                && text
                    .as_bytes()
                    .get(..2)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"41"))),
        "invalid source address length"
    );
    normalize_tron_address(text).context("invalid source address")
}
fn optional_u64(value: &Value, key: &str) -> Result<Option<u64>> {
    value
        .get(key)
        .map(|v| {
            let n = v
                .as_u64()
                .with_context(|| format!("invalid unsigned source integer: {key}"))?;
            ensure!(n <= i64::MAX as u64, "source int64 overflow: {key}");
            Ok(n)
        })
        .transpose()
}
fn optional_string<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>> {
    value
        .get(key)
        .map(|v| {
            v.as_str()
                .with_context(|| format!("invalid source string: {key}"))
        })
        .transpose()
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    match value.get(key) {
        None => Ok(&[]),
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .with_context(|| format!("invalid source array: {key}")),
    }
}
fn match_integer(proto: &Message<'_>, number: u32, value: &Value, key: &str) -> Result<()> {
    let original = proto.integer(number)?;
    ensure!(
        original <= i64::MAX as u64,
        "negative/overflowed protobuf int64: {key}"
    );
    let reported = optional_u64(value, key)?.unwrap_or(0);
    ensure!(
        original == reported,
        "protobuf/JSON integer mismatch: {key}"
    );
    Ok(())
}
fn match_bytes(
    proto: &Message<'_>,
    number: u32,
    value: &Value,
    key: &str,
    is_address: bool,
) -> Result<()> {
    let bytes = proto.bytes(number)?;
    let reported = match value.get(key) {
        None => Vec::new(),
        Some(v) if is_address => {
            hex::decode(base58_to_hex(&address(v)?).context("address conversion failed")?)?
        }
        Some(v) => hex::decode(hex_bytes(v)?)?,
    };
    if is_address {
        ensure!(
            bytes.len() == 21 && bytes[0] == 0x41,
            "invalid protobuf address: {key}"
        );
    }
    ensure!(bytes == reported, "protobuf/JSON bytes mismatch: {key}");
    Ok(())
}
fn verify_contract(bytes: &[u8], contract: &Value, genesis: bool) -> Result<ContractEvidence> {
    let proto = Message::parse(bytes)?;
    let type_id = u32::try_from(proto.integer(1)?)?;
    let name = contract["type"]
        .as_str()
        .context("contract type must be named")?;
    ensure!(
        contract_type_id(name) == Some(type_id),
        "protobuf/JSON contract type mismatch or unsupported type id"
    );
    match_integer(&proto, 5, contract, "Permission_id")?;
    ensure!(
        proto.integer(5)? <= i32::MAX as u64,
        "protobuf Permission_id overflow"
    );
    match_bytes(&proto, 3, contract, "provider", false)?;
    match_bytes(&proto, 4, contract, "ContractName", false)?;
    let any = Message::parse(proto.bytes(2)?)?;
    let type_url = std::str::from_utf8(any.bytes(1)?)?.to_owned();
    ensure!(
        type_url.rsplit('/').next() == Some(format!("protocol.{name}").as_str()),
        "protobuf contract type URL mismatch"
    );
    ensure!(
        contract["parameter"]["type_url"].as_str() == Some(type_url.as_str()),
        "protobuf/JSON parameter type URL mismatch"
    );
    let value_bytes = any.bytes(2)?;
    let value = &contract["parameter"]["value"];
    ensure!(value.is_object(), "contract parameter JSON missing");
    let fields_verified = matches!(type_id, 1 | 2 | 31);
    if fields_verified {
        let fields = Message::parse(value_bytes)?;
        match type_id {
            1 => {
                // java-tron's genesis allocation owner is a fixed byte marker,
                // not a wallet. Preserve it only at height zero; never make an edge.
                let genesis_marker = genesis && fields.bytes(1)? == b"0x000000000000000000000";
                match_bytes(&fields, 1, value, "owner_address", !genesis_marker)?;
                match_bytes(&fields, 2, value, "to_address", true)?;
                match_integer(&fields, 3, value, "amount")?;
            }
            2 => {
                match_bytes(&fields, 1, value, "asset_name", false)?;
                match_bytes(&fields, 2, value, "owner_address", true)?;
                match_bytes(&fields, 3, value, "to_address", true)?;
                match_integer(&fields, 4, value, "amount")?;
            }
            31 => {
                match_bytes(&fields, 1, value, "owner_address", true)?;
                match_bytes(&fields, 2, value, "contract_address", true)?;
                match_integer(&fields, 3, value, "call_value")?;
                match_bytes(&fields, 4, value, "data", false)?;
                match_integer(&fields, 5, value, "call_token_value")?;
                match_integer(&fields, 6, value, "token_id")?;
            }
            _ => unreachable!(),
        }
    }
    Ok(ContractEvidence {
        type_id,
        type_url,
        value_hex: hex::encode(value_bytes),
        fields_verified,
    })
}
fn contract_type_id(name: &str) -> Option<u32> {
    Some(match name {
        "AccountCreateContract" => 0,
        "TransferContract" => 1,
        "TransferAssetContract" => 2,
        "VoteAssetContract" => 3,
        "VoteWitnessContract" => 4,
        "WitnessCreateContract" => 5,
        "AssetIssueContract" => 6,
        "WitnessUpdateContract" => 8,
        "ParticipateAssetIssueContract" => 9,
        "AccountUpdateContract" => 10,
        "FreezeBalanceContract" => 11,
        "UnfreezeBalanceContract" => 12,
        "WithdrawBalanceContract" => 13,
        "UnfreezeAssetContract" => 14,
        "UpdateAssetContract" => 15,
        "ProposalCreateContract" => 16,
        "ProposalApproveContract" => 17,
        "ProposalDeleteContract" => 18,
        "SetAccountIdContract" => 19,
        "CustomContract" => 20,
        "CreateSmartContract" => 30,
        "TriggerSmartContract" => 31,
        "GetContract" => 32,
        "UpdateSettingContract" => 33,
        "ExchangeCreateContract" => 41,
        "ExchangeInjectContract" => 42,
        "ExchangeWithdrawContract" => 43,
        "ExchangeTransactionContract" => 44,
        "UpdateEnergyLimitContract" => 45,
        "AccountPermissionUpdateContract" => 46,
        "ClearABIContract" => 48,
        "UpdateBrokerageContract" => 49,
        "ShieldedTransferContract" => 51,
        "MarketSellAssetContract" => 52,
        "MarketCancelOrderContract" => 53,
        "FreezeBalanceV2Contract" => 54,
        "UnfreezeBalanceV2Contract" => 55,
        "WithdrawExpireUnfreezeContract" => 56,
        "DelegateResourceContract" => 57,
        "UnDelegateResourceContract" => 58,
        "CancelAllUnfreezeV2Contract" => 59,
        _ => return None,
    })
}
