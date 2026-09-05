use std::collections::HashMap;

use clickhouse::types::UInt256;
use serde::Deserialize;

use crate::BSC_NETWORK_ID;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcBlock {
    pub number: Option<String>,
    pub hash: Option<String>,
    pub parent_hash: String,
    pub timestamp: String,
    pub transactions: Vec<RpcTransaction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcTransaction {
    pub hash: String,
    pub block_hash: Option<String>,
    pub block_number: Option<String>,
    pub transaction_index: Option<String>,
    pub from: String,
    pub to: Option<String>,
    pub nonce: String,
    #[serde(rename = "type")]
    pub transaction_type: Option<String>,
    pub value: String,
    pub input: String,
    pub gas: String,
    pub gas_price: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcReceipt {
    pub transaction_hash: String,
    pub transaction_index: String,
    pub block_hash: String,
    pub block_number: String,
    pub from: String,
    pub to: Option<String>,
    pub contract_address: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub transaction_type: Option<String>,
    pub gas_used: String,
    pub effective_gas_price: Option<String>,
    pub logs: Vec<RpcLog>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcLog {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub block_number: String,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: String,
    pub log_index: String,
    pub removed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcBlockTrace {
    pub tx_hash: String,
    pub result: RpcCallFrame,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RpcCallFrame {
    #[serde(rename = "type")]
    pub call_type: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub value: Option<String>,
    pub error: Option<String>,
    #[serde(rename = "revertReason")]
    pub revert_reason: Option<String>,
    #[serde(default)]
    pub calls: Vec<RpcCallFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalBlock {
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp_unix_ms: u64,
    pub transactions: Vec<CanonicalTransaction>,
    pub logs: Vec<CanonicalLog>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalTransaction {
    pub hash: String,
    pub transaction_index: u32,
    pub from_address: String,
    pub to_address: String,
    pub contract_address: String,
    pub nonce: u64,
    pub transaction_type: u8,
    pub value: EvmU256,
    pub input_selector: String,
    pub input_data: String,
    pub status: u8,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub effective_gas_price: EvmU256,
    pub fee_paid: EvmU256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalLog {
    pub event_id: String,
    pub tx_hash: String,
    pub transaction_index: u32,
    pub log_index: u32,
    pub contract_address: String,
    pub topic0: String,
    pub topics: Vec<String>,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvmU256([u8; 32]);

impl EvmU256 {
    pub(crate) fn parse(field: &'static str, value: &str) -> Result<Self, DecodeError> {
        let hex = quantity_hex(field, value)?;
        if hex.len() > 64 {
            return Err(DecodeError::InvalidField {
                field,
                reason: "quantity exceeds 256 bits",
            });
        }
        let mut bytes = [0_u8; 32];
        let mut source_index = hex.len();
        let mut target_index = 0;
        while source_index > 0 {
            let low =
                hex_nibble(hex.as_bytes()[source_index - 1]).ok_or(DecodeError::InvalidField {
                    field,
                    reason: "quantity contains non-hex characters",
                })?;
            source_index -= 1;
            let high = if source_index > 0 {
                let value = hex_nibble(hex.as_bytes()[source_index - 1]).ok_or(
                    DecodeError::InvalidField {
                        field,
                        reason: "quantity contains non-hex characters",
                    },
                )?;
                source_index -= 1;
                value
            } else {
                0
            };
            bytes[target_index] = low | (high << 4);
            target_index += 1;
        }
        Ok(Self(bytes))
    }

    fn checked_mul_u64(self, rhs: u64) -> Option<Self> {
        let mut output = [0_u8; 32];
        let mut carry = 0_u128;
        for limb_index in 0..4 {
            let offset = limb_index * 8;
            let mut limb_bytes = [0_u8; 8];
            limb_bytes.copy_from_slice(&self.0[offset..offset + 8]);
            let product = u128::from(u64::from_le_bytes(limb_bytes)) * u128::from(rhs) + carry;
            output[offset..offset + 8].copy_from_slice(&(product as u64).to_le_bytes());
            carry = product >> 64;
        }
        (carry == 0).then_some(Self(output))
    }

    pub(crate) fn as_clickhouse(self) -> UInt256 {
        UInt256::from_le_bytes(self.0)
    }

    pub(crate) fn from_be_word(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut little_endian = [0_u8; 32];
        for (index, byte) in bytes.iter().rev().enumerate() {
            little_endian[index] = *byte;
        }
        Some(Self(little_endian))
    }

    pub(crate) fn one() -> Self {
        let mut bytes = [0_u8; 32];
        bytes[0] = 1;
        Self(bytes)
    }

    pub(crate) fn is_zero(self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }

    pub(crate) fn decimal_string(self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }

        let mut quotient = self.0;
        let mut digits = Vec::new();
        while quotient.iter().any(|byte| *byte != 0) {
            let mut remainder = 0_u16;
            for byte in quotient.iter_mut().rev() {
                let value = remainder * 256 + u16::from(*byte);
                *byte = u8::try_from(value / 10).expect("base-256 division quotient fits u8");
                remainder = value % 10;
            }
            digits.push(char::from(
                b'0' + u8::try_from(remainder).expect("decimal remainder fits u8"),
            ));
        }
        digits.iter().rev().collect()
    }

    fn as_u64(self, field: &'static str) -> Result<u64, DecodeError> {
        if self.0[8..].iter().any(|byte| *byte != 0) {
            return Err(DecodeError::InvalidField {
                field,
                reason: "quantity exceeds UInt64",
            });
        }
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&self.0[..8]);
        Ok(u64::from_le_bytes(bytes))
    }
}

pub(crate) fn normalize_block(
    block: RpcBlock,
    receipts: Vec<RpcReceipt>,
) -> Result<CanonicalBlock, DecodeError> {
    let number = parse_u64(
        "block.number",
        block
            .number
            .as_deref()
            .ok_or(DecodeError::MissingField("block.number"))?,
    )?;
    let hash = normalize_hash(
        "block.hash",
        block
            .hash
            .as_deref()
            .ok_or(DecodeError::MissingField("block.hash"))?,
    )?;
    let parent_hash = normalize_hash("block.parentHash", &block.parent_hash)?;
    let timestamp_seconds = parse_u64("block.timestamp", &block.timestamp)?;
    let timestamp_unix_ms =
        timestamp_seconds
            .checked_mul(1_000)
            .ok_or(DecodeError::InvalidField {
                field: "block.timestamp",
                reason: "millisecond timestamp overflows UInt64",
            })?;

    let mut receipts_by_hash = HashMap::with_capacity(receipts.len());
    for receipt in receipts {
        let tx_hash = normalize_hash("receipt.transactionHash", &receipt.transaction_hash)?;
        if receipts_by_hash.insert(tx_hash, receipt).is_some() {
            return Err(DecodeError::DuplicateReceipt);
        }
    }
    if receipts_by_hash.len() != block.transactions.len() {
        return Err(DecodeError::ReceiptCountMismatch {
            transactions: block.transactions.len(),
            receipts: receipts_by_hash.len(),
        });
    }

    let mut transactions = Vec::with_capacity(block.transactions.len());
    let mut logs = Vec::new();
    for (expected_index, transaction) in block.transactions.into_iter().enumerate() {
        let expected_index =
            u32::try_from(expected_index).map_err(|_| DecodeError::InvalidField {
                field: "block.transactions",
                reason: "transaction count exceeds UInt32",
            })?;
        let tx_hash = normalize_hash("transaction.hash", &transaction.hash)?;
        let receipt = receipts_by_hash
            .remove(&tx_hash)
            .ok_or(DecodeError::MissingReceipt)?;
        validate_block_reference(number, &hash, expected_index, &transaction, &receipt)?;

        let from_address = normalize_address("transaction.from", &transaction.from)?;
        let receipt_from = normalize_address("receipt.from", &receipt.from)?;
        if from_address != receipt_from {
            return Err(DecodeError::EvidenceMismatch("receipt.from"));
        }
        let to_address = normalize_optional_address("transaction.to", transaction.to.as_deref())?;
        let receipt_to = normalize_optional_address("receipt.to", receipt.to.as_deref())?;
        if to_address != receipt_to {
            return Err(DecodeError::EvidenceMismatch("receipt.to"));
        }
        let contract_address = normalize_optional_address(
            "receipt.contractAddress",
            receipt.contract_address.as_deref(),
        )?;
        if !to_address.is_empty() && !contract_address.is_empty() {
            return Err(DecodeError::EvidenceMismatch("receipt.contractAddress"));
        }

        let transaction_type = parse_u8(
            "transaction.type",
            transaction.transaction_type.as_deref().unwrap_or("0x0"),
        )?;
        if let Some(receipt_type) = receipt.transaction_type.as_deref()
            && parse_u8("receipt.type", receipt_type)? != transaction_type
        {
            return Err(DecodeError::EvidenceMismatch("receipt.type"));
        }
        let status = parse_u8(
            "receipt.status",
            receipt
                .status
                .as_deref()
                .ok_or(DecodeError::MissingField("receipt.status"))?,
        )?;
        if status > 1 {
            return Err(DecodeError::InvalidField {
                field: "receipt.status",
                reason: "status must be zero or one",
            });
        }
        let gas_limit = parse_u64("transaction.gas", &transaction.gas)?;
        let gas_used = parse_u64("receipt.gasUsed", &receipt.gas_used)?;
        if gas_used > gas_limit {
            return Err(DecodeError::EvidenceMismatch("receipt.gasUsed"));
        }
        let effective_gas_price = EvmU256::parse(
            "receipt.effectiveGasPrice",
            receipt
                .effective_gas_price
                .as_deref()
                .or(transaction.gas_price.as_deref())
                .ok_or(DecodeError::MissingField("receipt.effectiveGasPrice"))?,
        )?;
        let fee_paid =
            effective_gas_price
                .checked_mul_u64(gas_used)
                .ok_or(DecodeError::InvalidField {
                    field: "fee_paid",
                    reason: "gas fee exceeds UInt256",
                })?;
        let input_data = normalize_data("transaction.input", &transaction.input)?;
        let input_selector = if input_data.len() >= 10 {
            input_data[..10].to_string()
        } else {
            String::new()
        };

        for log in receipt.logs {
            logs.push(normalize_log(number, &hash, &tx_hash, expected_index, log)?);
        }
        transactions.push(CanonicalTransaction {
            hash: tx_hash,
            transaction_index: expected_index,
            from_address,
            to_address,
            contract_address,
            nonce: parse_u64("transaction.nonce", &transaction.nonce)?,
            transaction_type,
            value: EvmU256::parse("transaction.value", &transaction.value)?,
            input_selector,
            input_data,
            status,
            gas_limit,
            gas_used,
            effective_gas_price,
            fee_paid,
        });
    }
    if !receipts_by_hash.is_empty() {
        return Err(DecodeError::UnknownReceipt);
    }

    logs.sort_unstable_by_key(|log| log.log_index);
    for (expected, log) in logs.iter().enumerate() {
        let expected = u32::try_from(expected).map_err(|_| DecodeError::InvalidField {
            field: "receipt.logs",
            reason: "log count exceeds UInt32",
        })?;
        if log.log_index != expected {
            return Err(DecodeError::NonContiguousLogIndex {
                expected,
                actual: log.log_index,
            });
        }
    }

    Ok(CanonicalBlock {
        number,
        hash,
        parent_hash,
        timestamp_unix_ms,
        transactions,
        logs,
    })
}

fn validate_block_reference(
    block_number: u64,
    block_hash: &str,
    transaction_index: u32,
    transaction: &RpcTransaction,
    receipt: &RpcReceipt,
) -> Result<(), DecodeError> {
    let tx_block_hash = normalize_hash(
        "transaction.blockHash",
        transaction
            .block_hash
            .as_deref()
            .ok_or(DecodeError::MissingField("transaction.blockHash"))?,
    )?;
    let tx_block_number = parse_u64(
        "transaction.blockNumber",
        transaction
            .block_number
            .as_deref()
            .ok_or(DecodeError::MissingField("transaction.blockNumber"))?,
    )?;
    let tx_index = parse_u32(
        "transaction.transactionIndex",
        transaction
            .transaction_index
            .as_deref()
            .ok_or(DecodeError::MissingField("transaction.transactionIndex"))?,
    )?;
    let receipt_block_hash = normalize_hash("receipt.blockHash", &receipt.block_hash)?;
    let receipt_block_number = parse_u64("receipt.blockNumber", &receipt.block_number)?;
    let receipt_index = parse_u32("receipt.transactionIndex", &receipt.transaction_index)?;
    if tx_block_hash != block_hash
        || receipt_block_hash != block_hash
        || tx_block_number != block_number
        || receipt_block_number != block_number
        || tx_index != transaction_index
        || receipt_index != transaction_index
    {
        return Err(DecodeError::EvidenceMismatch("block/transaction reference"));
    }
    Ok(())
}

fn normalize_log(
    block_number: u64,
    block_hash: &str,
    tx_hash: &str,
    transaction_index: u32,
    log: RpcLog,
) -> Result<CanonicalLog, DecodeError> {
    if log.removed.unwrap_or(false) {
        return Err(DecodeError::InvalidField {
            field: "log.removed",
            reason: "finalized receipt contains a removed log",
        });
    }
    if parse_u64("log.blockNumber", &log.block_number)? != block_number
        || normalize_hash("log.blockHash", &log.block_hash)? != block_hash
        || normalize_hash("log.transactionHash", &log.transaction_hash)? != tx_hash
        || parse_u32("log.transactionIndex", &log.transaction_index)? != transaction_index
    {
        return Err(DecodeError::EvidenceMismatch(
            "log block/transaction reference",
        ));
    }
    let log_index = parse_u32("log.logIndex", &log.log_index)?;
    let topics = log
        .topics
        .iter()
        .map(|topic| normalize_hash("log.topic", topic))
        .collect::<Result<Vec<_>, _>>()?;
    let topic0 = topics.first().cloned().unwrap_or_default();
    Ok(CanonicalLog {
        event_id: format!("{BSC_NETWORK_ID}:{tx_hash}:{log_index}"),
        tx_hash: tx_hash.to_string(),
        transaction_index,
        log_index,
        contract_address: normalize_address("log.address", &log.address)?,
        topic0,
        topics,
        data: normalize_data("log.data", &log.data)?,
    })
}

fn parse_u64(field: &'static str, value: &str) -> Result<u64, DecodeError> {
    EvmU256::parse(field, value)?.as_u64(field)
}

fn parse_u32(field: &'static str, value: &str) -> Result<u32, DecodeError> {
    u32::try_from(parse_u64(field, value)?).map_err(|_| DecodeError::InvalidField {
        field,
        reason: "quantity exceeds UInt32",
    })
}

fn parse_u8(field: &'static str, value: &str) -> Result<u8, DecodeError> {
    u8::try_from(parse_u64(field, value)?).map_err(|_| DecodeError::InvalidField {
        field,
        reason: "quantity exceeds UInt8",
    })
}

fn quantity_hex<'a>(field: &'static str, value: &'a str) -> Result<&'a str, DecodeError> {
    let Some(hex) = value.strip_prefix("0x") else {
        return Err(DecodeError::InvalidField {
            field,
            reason: "quantity is not 0x-prefixed",
        });
    };
    if hex.is_empty() || (hex.len() > 1 && hex.starts_with('0')) {
        return Err(DecodeError::InvalidField {
            field,
            reason: "quantity is empty or has leading zeroes",
        });
    }
    Ok(hex)
}

pub(crate) fn normalize_hash(field: &'static str, value: &str) -> Result<String, DecodeError> {
    normalize_fixed_hex(field, value, 32)
}

pub(crate) fn normalize_address(field: &'static str, value: &str) -> Result<String, DecodeError> {
    normalize_fixed_hex(field, value, 20)
}

fn normalize_optional_address(
    field: &'static str,
    value: Option<&str>,
) -> Result<String, DecodeError> {
    value.map_or_else(
        || Ok(String::new()),
        |value| normalize_address(field, value),
    )
}

fn normalize_fixed_hex(
    field: &'static str,
    value: &str,
    bytes: usize,
) -> Result<String, DecodeError> {
    let expected_length = 2 + bytes * 2;
    if value.len() != expected_length
        || !value.starts_with("0x")
        || !value.as_bytes()[2..]
            .iter()
            .all(|character| hex_nibble(*character).is_some())
    {
        return Err(DecodeError::InvalidField {
            field,
            reason: "hex value has an invalid length or character",
        });
    }
    Ok(value.to_ascii_lowercase())
}

fn normalize_data(field: &'static str, value: &str) -> Result<String, DecodeError> {
    let Some(hex) = value.strip_prefix("0x") else {
        return Err(DecodeError::InvalidField {
            field,
            reason: "byte data is not 0x-prefixed",
        });
    };
    if hex.len() % 2 != 0
        || !hex
            .as_bytes()
            .iter()
            .all(|value| hex_nibble(*value).is_some())
    {
        return Err(DecodeError::InvalidField {
            field,
            reason: "byte data has odd length or non-hex characters",
        });
    }
    Ok(value.to_ascii_lowercase())
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("missing required RPC field {0}")]
    MissingField(&'static str),
    #[error("invalid RPC field {field}: {reason}")]
    InvalidField {
        field: &'static str,
        reason: &'static str,
    },
    #[error("transaction and receipt evidence disagree at {0}")]
    EvidenceMismatch(&'static str),
    #[error("receipt count {receipts} does not match transaction count {transactions}")]
    ReceiptCountMismatch {
        transactions: usize,
        receipts: usize,
    },
    #[error("duplicate receipt returned for a transaction")]
    DuplicateReceipt,
    #[error("receipt is unavailable for a finalized transaction")]
    MissingReceipt,
    #[error("receipt was returned for a transaction outside the block")]
    UnknownReceipt,
    #[error("log indexes are not contiguous: expected {expected}, received {actual}")]
    NonContiguousLogIndex { expected: u32, actual: u32 },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{EvmU256, RpcBlock, RpcReceipt, normalize_block};

    fn block_and_receipt() -> (RpcBlock, RpcReceipt) {
        let hash = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let tx_hash = "0x2222222222222222222222222222222222222222222222222222222222222222";
        let block = serde_json::from_value(json!({
            "number": "0x64",
            "hash": hash,
            "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000063",
            "timestamp": "0x6553f100",
            "transactions": [{
                "hash": tx_hash,
                "blockHash": hash,
                "blockNumber": "0x64",
                "transactionIndex": "0x0",
                "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "to": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "nonce": "0x1",
                "type": "0x2",
                "value": "0x0",
                "input": "0xa9059cbb",
                "gas": "0x5208",
                "gasPrice": "0x3b9aca00"
            }]
        }))
        .unwrap();
        let receipt = serde_json::from_value(json!({
            "transactionHash": tx_hash,
            "transactionIndex": "0x0",
            "blockHash": hash,
            "blockNumber": "0x64",
            "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "to": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "contractAddress": null,
            "status": "0x1",
            "type": "0x2",
            "gasUsed": "0x5208",
            "effectiveGasPrice": "0x3b9aca00",
            "logs": []
        }))
        .unwrap();
        (block, receipt)
    }

    #[test]
    fn normalizes_complete_receipt_and_exact_fee() {
        let (block, receipt) = block_and_receipt();
        let block = normalize_block(block, vec![receipt]).unwrap();
        assert_eq!(block.transactions.len(), 1);
        assert_eq!(block.transactions[0].input_selector, "0xa9059cbb");
        assert_eq!(
            block.transactions[0].fee_paid.as_clickhouse().to_string(),
            "21000000000000"
        );
    }

    #[test]
    fn rejects_missing_receipt_and_removed_log() {
        let (block, mut receipt) = block_and_receipt();
        assert!(normalize_block(block.clone(), Vec::new()).is_err());
        receipt.logs = serde_json::from_value(json!([{
            "address": "0xcccccccccccccccccccccccccccccccccccccccc",
            "topics": [],
            "data": "0x",
            "blockNumber": "0x64",
            "blockHash": "0x1111111111111111111111111111111111111111111111111111111111111111",
            "transactionHash": "0x2222222222222222222222222222222222222222222222222222222222222222",
            "transactionIndex": "0x0",
            "logIndex": "0x0",
            "removed": true
        }]))
        .unwrap();
        assert!(normalize_block(block, vec![receipt]).is_err());
    }

    #[test]
    fn uint256_multiplication_detects_overflow() {
        let maximum = EvmU256::parse(
            "test",
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        assert!(maximum.checked_mul_u64(2).is_none());
    }

    #[test]
    fn uint256_decimal_conversion_preserves_full_width_token_ids() {
        let maximum = EvmU256::parse(
            "token_id",
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        assert_eq!(
            maximum.decimal_string(),
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
    }

    #[test]
    fn real_bsc_edge_case_fixture_retains_failed_and_creation_evidence() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/bsc/real_edge_cases.json"
        ))
        .unwrap();
        assert_eq!(fixture["network"], "eip155:56");
        assert_eq!(fixture["cases"][0]["kind"], "failed");
        assert_eq!(fixture["cases"][0]["receipt"]["status"], "0x0");
        assert_eq!(fixture["cases"][1]["kind"], "contract_creation");
        assert!(fixture["cases"][1]["transaction"]["to"].is_null());
        assert_eq!(
            fixture["cases"][1]["receipt"]["contract_address"],
            "0x97c7686d4b86799dea318b2a727cef22445d5df5"
        );
    }
}
