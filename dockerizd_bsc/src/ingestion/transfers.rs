use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::{
    BSC_NETWORK_ID,
    domain::{AssetId, IdentifierError, NetworkId},
};

use super::model::{
    CanonicalBlock, CanonicalLog, CanonicalTransaction, DecodeError, EvmU256, RpcBlockTrace,
    RpcCallFrame, normalize_address, normalize_hash,
};

const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const ERC1155_TRANSFER_SINGLE_TOPIC: &str =
    "0xc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62";
const ERC1155_TRANSFER_BATCH_TOPIC: &str =
    "0x4a39dc06d4c0dbc64b70af90fd698a233a518aa5d07e595d983b8c0526c8f7fb";
const MAX_ERC1155_BATCH_ITEMS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedBlockEvidence {
    pub relationships: Vec<CanonicalRelationship>,
    pub token_discoveries: Vec<TokenMetadataDiscovery>,
    pub trace_data_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalRelationship {
    pub relationship_id: String,
    pub tx_hash: String,
    pub transaction_index: u32,
    pub event_index: u32,
    pub event_sub_index: u32,
    pub trace_address: Vec<u32>,
    pub from_address: String,
    pub to_address: String,
    pub asset_id: String,
    pub token_id: String,
    pub amount: EvmU256,
    pub transfer_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenMetadataDiscovery {
    pub discovery_id: String,
    pub token_address: String,
    pub standard_hint: &'static str,
    pub tx_hash: String,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenStandard {
    Erc20,
    Erc721,
    Erc1155,
}

impl TokenStandard {
    fn as_str(self) -> &'static str {
        match self {
            Self::Erc20 => "erc20",
            Self::Erc721 => "erc721",
            Self::Erc1155 => "erc1155",
        }
    }
}

#[derive(Debug)]
struct TokenMovement {
    from_address: String,
    to_address: String,
    amount: EvmU256,
    token_id: Option<EvmU256>,
    standard: TokenStandard,
    event_sub_index: u32,
}

pub(crate) fn extract_block_evidence(
    block: &CanonicalBlock,
    traces: Option<&[RpcBlockTrace]>,
) -> Result<ExtractedBlockEvidence, TransferDecodeError> {
    let mut relationships = Vec::new();
    let mut discoveries = HashMap::<(String, TokenStandard), TokenMetadataDiscovery>::new();

    for transaction in &block.transactions {
        if transaction.status == 1 {
            append_native_transfer(transaction, &mut relationships)?;
        } else if block
            .logs
            .iter()
            .any(|log| log.transaction_index == transaction.transaction_index)
        {
            return Err(TransferDecodeError::FailedTransactionHasLogs {
                tx_hash: transaction.hash.clone(),
            });
        }
    }

    for log in &block.logs {
        let transaction = block
            .transactions
            .get(log.transaction_index as usize)
            .ok_or_else(|| TransferDecodeError::UnknownLogTransaction {
                event_id: log.event_id.clone(),
            })?;
        if transaction.hash != log.tx_hash {
            return Err(TransferDecodeError::UnknownLogTransaction {
                event_id: log.event_id.clone(),
            });
        }

        let movements = match decode_token_log(log) {
            Ok(movements) => movements,
            Err(TransferDecodeError::MalformedTokenEvent { .. }) => continue,
            Err(error) => return Err(error),
        };
        for movement in movements {
            let standard = movement.standard;
            discoveries
                .entry((log.contract_address.clone(), standard))
                .or_insert_with(|| TokenMetadataDiscovery {
                    discovery_id: stable_id(&format!(
                        "{BSC_NETWORK_ID}|token|{}|{}",
                        log.contract_address,
                        standard.as_str()
                    )),
                    token_address: log.contract_address.clone(),
                    standard_hint: standard.as_str(),
                    tx_hash: log.tx_hash.clone(),
                    evidence_id: log.event_id.clone(),
                });

            if movement.amount.is_zero() || movement.from_address == movement.to_address {
                continue;
            }
            let token_id = movement
                .token_id
                .map(EvmU256::decimal_string)
                .unwrap_or_default();
            let asset_id = token_asset(standard, &log.contract_address, &token_id)?;
            relationships.push(relationship(
                &log.tx_hash,
                log.transaction_index,
                log.log_index,
                movement.event_sub_index,
                Vec::new(),
                movement.from_address,
                movement.to_address,
                asset_id,
                token_id,
                movement.amount,
                standard.as_str(),
            ));
        }
    }

    if let Some(traces) = traces {
        append_internal_transfers(block, traces, &mut relationships)?;
    }

    relationships.sort_unstable_by(|left, right| {
        (
            left.transaction_index,
            left.event_index,
            left.event_sub_index,
            &left.trace_address,
            &left.relationship_id,
        )
            .cmp(&(
                right.transaction_index,
                right.event_index,
                right.event_sub_index,
                &right.trace_address,
                &right.relationship_id,
            ))
    });
    let mut token_discoveries = discoveries.into_values().collect::<Vec<_>>();
    token_discoveries.sort_unstable_by(|left, right| {
        (&left.token_address, left.standard_hint).cmp(&(&right.token_address, right.standard_hint))
    });

    Ok(ExtractedBlockEvidence {
        relationships,
        token_discoveries,
        trace_data_complete: traces.is_some(),
    })
}

fn append_native_transfer(
    transaction: &CanonicalTransaction,
    relationships: &mut Vec<CanonicalRelationship>,
) -> Result<(), TransferDecodeError> {
    if transaction.value.is_zero() {
        return Ok(());
    }
    let destination = if transaction.to_address.is_empty() {
        &transaction.contract_address
    } else {
        &transaction.to_address
    };
    if destination.is_empty() {
        return Err(TransferDecodeError::MissingNativeDestination {
            tx_hash: transaction.hash.clone(),
        });
    }
    if transaction.from_address == *destination {
        return Ok(());
    }
    relationships.push(relationship(
        &transaction.hash,
        transaction.transaction_index,
        transaction.transaction_index,
        0,
        Vec::new(),
        transaction.from_address.clone(),
        destination.clone(),
        AssetId::native_bnb().canonical_key(),
        String::new(),
        transaction.value,
        "native",
    ));
    Ok(())
}

fn append_internal_transfers(
    block: &CanonicalBlock,
    traces: &[RpcBlockTrace],
    relationships: &mut Vec<CanonicalRelationship>,
) -> Result<(), TransferDecodeError> {
    if traces.len() != block.transactions.len() {
        return Err(TransferDecodeError::TraceCountMismatch {
            transactions: block.transactions.len(),
            traces: traces.len(),
        });
    }

    let mut by_hash = HashMap::with_capacity(traces.len());
    for trace in traces {
        let tx_hash = normalize_hash("trace.txHash", &trace.tx_hash)?;
        if by_hash.insert(tx_hash, trace).is_some() {
            return Err(TransferDecodeError::DuplicateTrace);
        }
    }

    for transaction in &block.transactions {
        let trace =
            by_hash
                .remove(&transaction.hash)
                .ok_or_else(|| TransferDecodeError::MissingTrace {
                    tx_hash: transaction.hash.clone(),
                })?;
        let root_from =
            trace
                .result
                .from
                .as_deref()
                .ok_or_else(|| TransferDecodeError::InvalidTrace {
                    tx_hash: transaction.hash.clone(),
                    reason: "root frame has no from address",
                })?;
        if normalize_address("trace.root.from", root_from)? != transaction.from_address {
            return Err(TransferDecodeError::InvalidTrace {
                tx_hash: transaction.hash.clone(),
                reason: "root sender does not match transaction sender",
            });
        }

        if transaction.status == 1 {
            if frame_reverted(&trace.result) {
                return Err(TransferDecodeError::InvalidTrace {
                    tx_hash: transaction.hash.clone(),
                    reason: "successful receipt has a reverted root trace",
                });
            }
            for (index, child) in trace.result.calls.iter().enumerate() {
                append_call_frame(
                    transaction,
                    child,
                    vec![to_u32(index, "trace child index")?],
                    relationships,
                )?;
            }
        }
    }
    if !by_hash.is_empty() {
        return Err(TransferDecodeError::UnknownTrace);
    }
    Ok(())
}

fn append_call_frame(
    transaction: &CanonicalTransaction,
    frame: &RpcCallFrame,
    trace_address: Vec<u32>,
    relationships: &mut Vec<CanonicalRelationship>,
) -> Result<(), TransferDecodeError> {
    if frame_reverted(frame) {
        return Ok(());
    }

    let call_type = frame.call_type.trim().to_ascii_uppercase();
    let carries_value = match call_type.as_str() {
        "CALL" | "CREATE" | "CREATE2" | "SELFDESTRUCT" => true,
        "DELEGATECALL" | "STATICCALL" | "CALLCODE" => false,
        _ => {
            return Err(TransferDecodeError::InvalidTrace {
                tx_hash: transaction.hash.clone(),
                reason: "trace contains an unsupported call type",
            });
        }
    };

    if carries_value {
        let value = frame
            .value
            .as_deref()
            .map(|value| EvmU256::parse("trace.value", value))
            .transpose()?
            .unwrap_or_else(|| EvmU256::parse("trace.value", "0x0").expect("zero is valid"));
        if !value.is_zero() {
            let from_address = normalize_address(
                "trace.from",
                frame
                    .from
                    .as_deref()
                    .ok_or_else(|| TransferDecodeError::InvalidTrace {
                        tx_hash: transaction.hash.clone(),
                        reason: "value-carrying frame has no from address",
                    })?,
            )?;
            let to_address = normalize_address(
                "trace.to",
                frame
                    .to
                    .as_deref()
                    .ok_or_else(|| TransferDecodeError::InvalidTrace {
                        tx_hash: transaction.hash.clone(),
                        reason: "value-carrying frame has no to address",
                    })?,
            )?;
            if from_address != to_address {
                relationships.push(relationship(
                    &transaction.hash,
                    transaction.transaction_index,
                    *trace_address.first().unwrap_or(&0),
                    0,
                    trace_address.clone(),
                    from_address,
                    to_address,
                    AssetId::native_bnb().canonical_key(),
                    String::new(),
                    value,
                    "internal",
                ));
            }
        }
    }

    for (index, child) in frame.calls.iter().enumerate() {
        let mut child_address = trace_address.clone();
        child_address.push(to_u32(index, "trace child index")?);
        append_call_frame(transaction, child, child_address, relationships)?;
    }
    Ok(())
}

fn frame_reverted(frame: &RpcCallFrame) -> bool {
    frame
        .error
        .as_deref()
        .is_some_and(|value| !value.is_empty())
        || frame
            .revert_reason
            .as_deref()
            .is_some_and(|value| !value.is_empty())
}

fn decode_token_log(log: &CanonicalLog) -> Result<Vec<TokenMovement>, TransferDecodeError> {
    match log.topic0.as_str() {
        TRANSFER_TOPIC => decode_erc20_or_erc721(log).map(|movement| vec![movement]),
        ERC1155_TRANSFER_SINGLE_TOPIC => decode_erc1155_single(log).map(|movement| vec![movement]),
        ERC1155_TRANSFER_BATCH_TOPIC => decode_erc1155_batch(log),
        _ => Ok(Vec::new()),
    }
}

fn decode_erc20_or_erc721(log: &CanonicalLog) -> Result<TokenMovement, TransferDecodeError> {
    let from_address = topic_address(log, 1)?;
    let to_address = topic_address(log, 2)?;
    let data =
        decode_hex_data(&log.data).ok_or_else(|| malformed_event(log, "invalid data hex"))?;
    match (log.topics.len(), data.len()) {
        (3, 32) => Ok(TokenMovement {
            from_address,
            to_address,
            amount: word_u256(&data, 0).ok_or_else(|| malformed_event(log, "missing amount"))?,
            token_id: None,
            standard: TokenStandard::Erc20,
            event_sub_index: 0,
        }),
        (4, 0) => Ok(TokenMovement {
            from_address,
            to_address,
            amount: EvmU256::one(),
            token_id: Some(topic_u256(log, 3)?),
            standard: TokenStandard::Erc721,
            event_sub_index: 0,
        }),
        _ => Err(malformed_event(
            log,
            "Transfer event shape is neither ERC-20 nor ERC-721",
        )),
    }
}

fn decode_erc1155_single(log: &CanonicalLog) -> Result<TokenMovement, TransferDecodeError> {
    if log.topics.len() != 4 {
        return Err(malformed_event(log, "TransferSingle requires four topics"));
    }
    let data =
        decode_hex_data(&log.data).ok_or_else(|| malformed_event(log, "invalid data hex"))?;
    if data.len() != 64 {
        return Err(malformed_event(
            log,
            "TransferSingle requires two ABI words",
        ));
    }
    Ok(TokenMovement {
        from_address: topic_address(log, 2)?,
        to_address: topic_address(log, 3)?,
        token_id: Some(word_u256(&data, 0).expect("validated word exists")),
        amount: word_u256(&data, 1).expect("validated word exists"),
        standard: TokenStandard::Erc1155,
        event_sub_index: 0,
    })
}

fn decode_erc1155_batch(log: &CanonicalLog) -> Result<Vec<TokenMovement>, TransferDecodeError> {
    if log.topics.len() != 4 {
        return Err(malformed_event(log, "TransferBatch requires four topics"));
    }
    let from_address = topic_address(log, 2)?;
    let to_address = topic_address(log, 3)?;
    let data =
        decode_hex_data(&log.data).ok_or_else(|| malformed_event(log, "invalid data hex"))?;
    if data.len() < 64 {
        return Err(malformed_event(log, "TransferBatch ABI head is missing"));
    }
    let ids_offset =
        word_usize(&data, 0).ok_or_else(|| malformed_event(log, "invalid ids offset"))?;
    let values_offset =
        word_usize(&data, 1).ok_or_else(|| malformed_event(log, "invalid values offset"))?;
    let ids = abi_u256_array(&data, ids_offset, log)?;
    let values = abi_u256_array(&data, values_offset, log)?;
    if ids.len() != values.len() {
        return Err(malformed_event(log, "ids and values lengths differ"));
    }
    ids.into_iter()
        .zip(values)
        .enumerate()
        .map(|(index, (token_id, amount))| {
            Ok(TokenMovement {
                from_address: from_address.clone(),
                to_address: to_address.clone(),
                amount,
                token_id: Some(token_id),
                standard: TokenStandard::Erc1155,
                event_sub_index: to_u32(index, "ERC-1155 batch index")?,
            })
        })
        .collect()
}

fn abi_u256_array(
    data: &[u8],
    offset: usize,
    log: &CanonicalLog,
) -> Result<Vec<EvmU256>, TransferDecodeError> {
    if offset < 64 || !offset.is_multiple_of(32) {
        return Err(malformed_event(log, "array offset is invalid"));
    }
    let length = word_usize_at(data, offset)
        .ok_or_else(|| malformed_event(log, "array length is invalid"))?;
    if length > MAX_ERC1155_BATCH_ITEMS {
        return Err(malformed_event(log, "ERC-1155 batch exceeds safety limit"));
    }
    let start = offset
        .checked_add(32)
        .ok_or_else(|| malformed_event(log, "array offset overflow"))?;
    let byte_length = length
        .checked_mul(32)
        .ok_or_else(|| malformed_event(log, "array length overflow"))?;
    let end = start
        .checked_add(byte_length)
        .ok_or_else(|| malformed_event(log, "array end overflow"))?;
    let items = data
        .get(start..end)
        .ok_or_else(|| malformed_event(log, "array extends beyond event data"))?;
    Ok(items
        .chunks_exact(32)
        .map(|word| EvmU256::from_be_word(word).expect("chunk is one ABI word"))
        .collect())
}

fn topic_address(log: &CanonicalLog, index: usize) -> Result<String, TransferDecodeError> {
    let topic = log
        .topics
        .get(index)
        .and_then(|value| decode_hex_data(value))
        .ok_or_else(|| malformed_event(log, "indexed address topic is missing"))?;
    if topic.len() != 32 || topic[..12].iter().any(|byte| *byte != 0) {
        return Err(malformed_event(
            log,
            "indexed address topic is not ABI padded",
        ));
    }
    Ok(format!("0x{}", encode_hex(&topic[12..])))
}

fn topic_u256(log: &CanonicalLog, index: usize) -> Result<EvmU256, TransferDecodeError> {
    log.topics
        .get(index)
        .and_then(|value| decode_hex_data(value))
        .as_deref()
        .and_then(EvmU256::from_be_word)
        .ok_or_else(|| malformed_event(log, "token id topic is missing"))
}

fn word_u256(data: &[u8], index: usize) -> Option<EvmU256> {
    let start = index.checked_mul(32)?;
    EvmU256::from_be_word(data.get(start..start.checked_add(32)?)?)
}

fn word_usize(data: &[u8], index: usize) -> Option<usize> {
    word_usize_at(data, index.checked_mul(32)?)
}

fn word_usize_at(data: &[u8], offset: usize) -> Option<usize> {
    let word = data.get(offset..offset.checked_add(32)?)?;
    if word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    let mut tail = [0_u8; 8];
    tail.copy_from_slice(&word[24..]);
    usize::try_from(u64::from_be_bytes(tail)).ok()
}

fn token_asset(
    standard: TokenStandard,
    contract: &str,
    token_id: &str,
) -> Result<String, IdentifierError> {
    let network = NetworkId::bsc_mainnet();
    Ok(match standard {
        TokenStandard::Erc20 => AssetId::erc20(network, contract)?,
        TokenStandard::Erc721 => AssetId::erc721(network, contract, token_id)?,
        TokenStandard::Erc1155 => AssetId::erc1155(network, contract, token_id)?,
    }
    .canonical_key())
}

#[allow(clippy::too_many_arguments)]
fn relationship(
    tx_hash: &str,
    transaction_index: u32,
    event_index: u32,
    event_sub_index: u32,
    trace_address: Vec<u32>,
    from_address: String,
    to_address: String,
    asset_id: String,
    token_id: String,
    amount: EvmU256,
    transfer_type: &'static str,
) -> CanonicalRelationship {
    let trace_key = trace_address
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let relationship_id = stable_id(&format!(
        "{BSC_NETWORK_ID}|{tx_hash}|{transaction_index}|{event_index}|{event_sub_index}|{trace_key}|{from_address}|{to_address}|{asset_id}|{token_id}|{transfer_type}"
    ));
    CanonicalRelationship {
        relationship_id,
        tx_hash: tx_hash.to_string(),
        transaction_index,
        event_index,
        event_sub_index,
        trace_address,
        from_address,
        to_address,
        asset_id,
        token_id,
        amount,
        transfer_type,
    }
}

fn malformed_event(log: &CanonicalLog, reason: &'static str) -> TransferDecodeError {
    TransferDecodeError::MalformedTokenEvent {
        event_id: log.event_id.clone(),
        reason,
    }
}

fn decode_hex_data(value: &str) -> Option<Vec<u8>> {
    let hex = value.strip_prefix("0x")?;
    if hex.len() % 2 != 0 {
        return None;
    }
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?))
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn stable_id(identity: &str) -> String {
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn to_u32(value: usize, field: &'static str) -> Result<u32, TransferDecodeError> {
    u32::try_from(value).map_err(|_| TransferDecodeError::IndexOverflow { field })
}

#[derive(Debug, thiserror::Error)]
pub enum TransferDecodeError {
    #[error(transparent)]
    Evidence(#[from] DecodeError),
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    #[error("failed transaction {tx_hash} unexpectedly contains logs")]
    FailedTransactionHasLogs { tx_hash: String },
    #[error("log {event_id} does not reference its canonical transaction")]
    UnknownLogTransaction { event_id: String },
    #[error("malformed token event {event_id}: {reason}")]
    MalformedTokenEvent {
        event_id: String,
        reason: &'static str,
    },
    #[error("successful value transaction {tx_hash} has no destination")]
    MissingNativeDestination { tx_hash: String },
    #[error("trace count {traces} does not match transaction count {transactions}")]
    TraceCountMismatch { transactions: usize, traces: usize },
    #[error("duplicate transaction trace returned by RPC")]
    DuplicateTrace,
    #[error("trace is unavailable for transaction {tx_hash}")]
    MissingTrace { tx_hash: String },
    #[error("RPC returned a trace for a transaction outside the block")]
    UnknownTrace,
    #[error("invalid trace for transaction {tx_hash}: {reason}")]
    InvalidTrace {
        tx_hash: String,
        reason: &'static str,
    },
    #[error("{field} exceeds UInt32")]
    IndexOverflow { field: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{CanonicalBlock, CanonicalLog, CanonicalTransaction};

    const HASH: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    const TX: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";
    const FROM: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TO: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const TOKEN: &str = "0xcccccccccccccccccccccccccccccccccccccccc";

    fn quantity(value: &str) -> EvmU256 {
        EvmU256::parse("test", value).unwrap()
    }

    fn transaction(value: &str) -> CanonicalTransaction {
        CanonicalTransaction {
            hash: TX.to_string(),
            transaction_index: 0,
            from_address: FROM.to_string(),
            to_address: TO.to_string(),
            contract_address: String::new(),
            nonce: 0,
            transaction_type: 2,
            value: quantity(value),
            input_selector: String::new(),
            input_data: "0x".to_string(),
            status: 1,
            gas_limit: 21_000,
            gas_used: 21_000,
            effective_gas_price: quantity("0x1"),
            fee_paid: quantity("0x5208"),
        }
    }

    fn block(logs: Vec<CanonicalLog>) -> CanonicalBlock {
        CanonicalBlock {
            number: 7,
            hash: HASH.to_string(),
            parent_hash: HASH.to_string(),
            timestamp_unix_ms: 1_700_000_000_000,
            transactions: vec![transaction("0x2a")],
            logs,
        }
    }

    fn topic_address_value(address: &str) -> String {
        format!("0x{}{}", "0".repeat(24), address.trim_start_matches("0x"))
    }

    fn word(value: u64) -> String {
        format!("{value:064x}")
    }

    fn log(topic0: &str, topics: Vec<String>, data: String) -> CanonicalLog {
        CanonicalLog {
            event_id: format!("{BSC_NETWORK_ID}:{TX}:0"),
            tx_hash: TX.to_string(),
            transaction_index: 0,
            log_index: 0,
            contract_address: TOKEN.to_string(),
            topic0: topic0.to_string(),
            topics,
            data,
        }
    }

    #[test]
    fn extracts_native_and_erc20_edges_with_discovery() {
        let transfer = log(
            TRANSFER_TOPIC,
            vec![
                TRANSFER_TOPIC.to_string(),
                topic_address_value(FROM),
                topic_address_value(TO),
            ],
            format!("0x{}", word(99)),
        );
        let evidence = extract_block_evidence(&block(vec![transfer]), None).unwrap();

        assert_eq!(evidence.relationships.len(), 2);
        let native = evidence
            .relationships
            .iter()
            .find(|relationship| relationship.transfer_type == "native")
            .unwrap();
        let erc20 = evidence
            .relationships
            .iter()
            .find(|relationship| relationship.transfer_type == "erc20")
            .unwrap();
        assert_eq!(native.amount.decimal_string(), "42");
        assert_eq!(erc20.amount.decimal_string(), "99");
        assert_eq!(evidence.token_discoveries.len(), 1);
        assert!(!evidence.trace_data_complete);
    }

    #[test]
    fn decodes_erc721_and_erc1155_batch_token_ids() {
        let erc721 = log(
            TRANSFER_TOPIC,
            vec![
                TRANSFER_TOPIC.to_string(),
                topic_address_value(FROM),
                topic_address_value(TO),
                format!("0x{}", word(42)),
            ],
            "0x".to_string(),
        );
        let movement = decode_token_log(&erc721).unwrap().remove(0);
        assert_eq!(movement.standard, TokenStandard::Erc721);
        assert_eq!(movement.token_id.unwrap().decimal_string(), "42");

        let single = log(
            ERC1155_TRANSFER_SINGLE_TOPIC,
            vec![
                ERC1155_TRANSFER_SINGLE_TOPIC.to_string(),
                topic_address_value(FROM),
                topic_address_value(FROM),
                topic_address_value(TO),
            ],
            format!("0x{}{}", word(6), word(60)),
        );
        let movement = decode_token_log(&single).unwrap().remove(0);
        assert_eq!(movement.standard, TokenStandard::Erc1155);
        assert_eq!(movement.token_id.unwrap().decimal_string(), "6");
        assert_eq!(movement.amount.decimal_string(), "60");

        let batch_data = format!(
            "0x{}{}{}{}{}{}{}{}",
            word(64),
            word(160),
            word(2),
            word(7),
            word(8),
            word(2),
            word(70),
            word(80)
        );
        let batch = log(
            ERC1155_TRANSFER_BATCH_TOPIC,
            vec![
                ERC1155_TRANSFER_BATCH_TOPIC.to_string(),
                topic_address_value(FROM),
                topic_address_value(FROM),
                topic_address_value(TO),
            ],
            batch_data,
        );
        let movements = decode_token_log(&batch).unwrap();
        assert_eq!(movements.len(), 2);
        assert_eq!(movements[1].token_id.unwrap().decimal_string(), "8");
        assert_eq!(movements[1].amount.decimal_string(), "80");
        assert_eq!(movements[1].event_sub_index, 1);
    }

    #[test]
    fn call_trace_emits_only_successful_balance_movements() {
        let traces = vec![RpcBlockTrace {
            tx_hash: TX.to_string(),
            result: RpcCallFrame {
                call_type: "CALL".to_string(),
                from: Some(FROM.to_string()),
                to: Some(TO.to_string()),
                value: Some("0x2a".to_string()),
                error: None,
                revert_reason: None,
                calls: vec![
                    RpcCallFrame {
                        call_type: "CALL".to_string(),
                        from: Some(TO.to_string()),
                        to: Some(TOKEN.to_string()),
                        value: Some("0x5".to_string()),
                        error: None,
                        revert_reason: None,
                        calls: Vec::new(),
                    },
                    RpcCallFrame {
                        call_type: "CREATE2".to_string(),
                        from: Some(TO.to_string()),
                        to: Some(TOKEN.to_string()),
                        value: Some("0x2".to_string()),
                        error: None,
                        revert_reason: None,
                        calls: Vec::new(),
                    },
                    RpcCallFrame {
                        call_type: "SELFDESTRUCT".to_string(),
                        from: Some(TOKEN.to_string()),
                        to: Some(FROM.to_string()),
                        value: Some("0x3".to_string()),
                        error: None,
                        revert_reason: None,
                        calls: Vec::new(),
                    },
                    RpcCallFrame {
                        call_type: "DELEGATECALL".to_string(),
                        from: Some(TO.to_string()),
                        to: Some(TOKEN.to_string()),
                        value: Some("0x5".to_string()),
                        error: None,
                        revert_reason: None,
                        calls: Vec::new(),
                    },
                    RpcCallFrame {
                        call_type: "CALL".to_string(),
                        from: Some(TO.to_string()),
                        to: Some(TOKEN.to_string()),
                        value: Some("0x7".to_string()),
                        error: Some("execution reverted".to_string()),
                        revert_reason: None,
                        calls: Vec::new(),
                    },
                ],
            },
        }];
        let evidence = extract_block_evidence(&block(Vec::new()), Some(&traces)).unwrap();
        assert!(evidence.trace_data_complete);
        assert_eq!(evidence.relationships.len(), 4);
        let internal = evidence
            .relationships
            .iter()
            .filter(|relationship| relationship.transfer_type == "internal")
            .collect::<Vec<_>>();
        assert_eq!(internal.len(), 3);
        assert!(
            internal
                .iter()
                .any(|relationship| relationship.trace_address == vec![0])
        );
    }

    #[test]
    fn preserves_zero_address_as_mint_evidence() {
        let zero = "0x0000000000000000000000000000000000000000";
        let mint = log(
            TRANSFER_TOPIC,
            vec![
                TRANSFER_TOPIC.to_string(),
                topic_address_value(zero),
                topic_address_value(TO),
            ],
            format!("0x{}", word(5)),
        );
        let evidence = extract_block_evidence(&block(vec![mint]), None).unwrap();
        let mint_edge = evidence
            .relationships
            .iter()
            .find(|relationship| relationship.transfer_type == "erc20")
            .unwrap();
        assert_eq!(mint_edge.from_address, zero);
        assert_eq!(mint_edge.to_address, TO);
    }

    #[test]
    fn malformed_standard_event_remains_raw_without_creating_an_edge() {
        let malformed = log(
            TRANSFER_TOPIC,
            vec![TRANSFER_TOPIC.to_string(), topic_address_value(FROM)],
            "0x".to_string(),
        );
        let evidence = extract_block_evidence(&block(vec![malformed]), None).unwrap();
        assert_eq!(evidence.relationships.len(), 1);
        assert_eq!(evidence.relationships[0].transfer_type, "native");
        assert!(evidence.token_discoveries.is_empty());
    }
}
