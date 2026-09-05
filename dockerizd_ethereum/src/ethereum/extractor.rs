use std::{collections::HashMap, str::FromStr, sync::LazyLock};

use alloy::{
    consensus::Transaction as _,
    network::TransactionResponse as _,
    primitives::{Address, B256, U256, keccak256},
    rpc::types::{Block, Log, TransactionReceipt},
};
use anyhow::{Context, anyhow, ensure};
use clickhouse::types::UInt256;
use sha2::{Digest, Sha256};

use crate::{
    config::AppConfig,
    domain::{AddressId, AssetId, CanonicalTransfer, NetworkId, TransferKind, TransferOrigin},
    storage::{
        AddressRelationshipRow, EvmLogRow, IngestedBlockRow, SemanticAmlEventRow,
        TokenMetadataDiscoveryRow, TokenMetadataJobRow, TransactionFeatureRow, TransactionRow,
    },
};

use super::{
    TransactionTrace,
    semantic::{
        ObservedMovement, SemanticDecoderRegistry, SemanticTransaction, decode_transaction,
    },
};

static ERC_TRANSFER_EVENT: LazyLock<B256> =
    LazyLock::new(|| keccak256("Transfer(address,address,uint256)"));
static ERC1155_TRANSFER_SINGLE_EVENT: LazyLock<B256> =
    LazyLock::new(|| keccak256("TransferSingle(address,address,address,uint256,uint256)"));
static ERC1155_TRANSFER_BATCH_EVENT: LazyLock<B256> =
    LazyLock::new(|| keccak256("TransferBatch(address,address,address,uint256[],uint256[])"));

#[derive(Debug)]
pub struct ExtractedBlock {
    pub block: IngestedBlockRow,
    pub transactions: Vec<TransactionRow>,
    pub logs: Vec<EvmLogRow>,
    pub relationships: Vec<AddressRelationshipRow>,
    pub transaction_features: Vec<TransactionFeatureRow>,
    pub semantic_events: Vec<SemanticAmlEventRow>,
    pub token_discoveries: Vec<TokenMetadataDiscoveryRow>,
    pub token_jobs: Vec<TokenMetadataJobRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
struct TokenMovement {
    contract: Address,
    from: Address,
    to: Address,
    amount: U256,
    token_id: Option<U256>,
    standard: TokenStandard,
    event_sub_index: u32,
}

#[allow(clippy::too_many_arguments)]
pub fn extract_block(
    config: &AppConfig,
    block: &Block,
    receipts: &[TransactionReceipt],
    traces: Option<&[TransactionTrace]>,
    semantic_registry: &SemanticDecoderRegistry,
    indexed_at_unix_ms: u64,
) -> anyhow::Result<ExtractedBlock> {
    let network = NetworkId::from_str(&config.eth_network_id)?;
    let block_number = block.header.number;
    let block_hash = block.header.hash;
    let block_timestamp_unix_ms = block
        .header
        .timestamp
        .checked_mul(1_000)
        .ok_or_else(|| anyhow!("block {block_number} timestamp overflows milliseconds"))?;
    let transactions = block
        .transactions
        .as_transactions()
        .ok_or_else(|| anyhow!("block {block_number} did not contain full transactions"))?;

    ensure!(
        transactions.len() == receipts.len(),
        "block {block_number} returned {} transactions but {} receipts",
        transactions.len(),
        receipts.len()
    );

    let mut receipts_by_hash = receipts
        .iter()
        .map(|receipt| (receipt.transaction_hash, receipt))
        .collect::<HashMap<_, _>>();
    ensure!(
        receipts_by_hash.len() == receipts.len(),
        "block {block_number} returned duplicate transaction receipts"
    );

    let traces_by_hash = traces
        .map(|traces| {
            traces
                .iter()
                .map(|trace| (trace.tx_hash, trace))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    if let Some(traces) = traces {
        ensure!(
            traces_by_hash.len() == traces.len(),
            "block {block_number} returned duplicate transaction traces"
        );
        ensure!(
            traces.len() == transactions.len(),
            "block {block_number} returned {} traces for {} transactions",
            traces.len(),
            transactions.len()
        );
    }

    let mut transaction_rows = Vec::with_capacity(transactions.len());
    let mut log_rows = Vec::new();
    let mut relationship_rows = Vec::new();
    let mut transaction_features = Vec::new();
    let mut semantic_events = Vec::new();
    let mut discovered_tokens = HashMap::<String, TokenStandard>::new();

    for transaction in transactions {
        let tx_hash = transaction.tx_hash();
        let receipt = receipts_by_hash
            .remove(&tx_hash)
            .with_context(|| format!("receipt missing for transaction {tx_hash:#x}"))?;
        validate_receipt(
            block_number,
            block_hash,
            transaction.transaction_index,
            receipt,
        )?;

        let transaction_index = to_u32(
            transaction.transaction_index.unwrap_or_default(),
            "transaction index",
        )?;
        let input = transaction.input();
        let input_selector = input.get(..4).map(hex_bytes).unwrap_or_default();
        let status = u8::from(receipt.status());
        let fee_paid = U256::from(receipt.gas_used) * U256::from(receipt.effective_gas_price);
        let destination = transaction.to().or(receipt.contract_address);
        let relationship_start = relationship_rows.len();
        let mut observed_movements = Vec::new();

        transaction_rows.push(TransactionRow {
            network_id: config.eth_network_id.clone(),
            tx_hash: format!("{tx_hash:#x}"),
            block_hash: format!("{block_hash:#x}"),
            block_number,
            block_timestamp_unix_ms,
            transaction_index,
            from_address: format!("{:#x}", transaction.from()),
            to_address: transaction
                .to()
                .map(|address| format!("{address:#x}"))
                .unwrap_or_default(),
            contract_address: receipt
                .contract_address
                .map(|address| format!("{address:#x}"))
                .unwrap_or_default(),
            nonce: transaction.nonce(),
            transaction_type: transaction.transaction_type().unwrap_or_default(),
            value: clickhouse_u256(transaction.value()),
            input_selector: input_selector.clone(),
            input_data: hex_bytes(input.as_ref()),
            status,
            status_known: 1,
            gas_limit: transaction.gas_limit(),
            gas_used: receipt.gas_used,
            effective_gas_price: UInt256::from(receipt.effective_gas_price),
            fee_paid: clickhouse_u256(fee_paid),
            max_fee_per_gas: UInt256::from(alloy::consensus::Transaction::max_fee_per_gas(
                transaction,
            )),
            max_priority_fee_per_gas: UInt256::from(
                transaction.max_priority_fee_per_gas().unwrap_or_default(),
            ),
            blob_gas_used: receipt.blob_gas_used.unwrap_or_default(),
            blob_gas_price: UInt256::from(receipt.blob_gas_price.unwrap_or_default()),
        });

        if receipt.status() && !transaction.value().is_zero() {
            if let Some(to) = destination {
                relationship_rows.push(relationship_row(CanonicalTransfer::new(
                    network.clone(),
                    block_hash,
                    block_number,
                    block_timestamp_unix_ms,
                    TransferOrigin::transaction(tx_hash, transaction_index),
                    AddressId::parse_evm(network.clone(), &format!("{:#x}", transaction.from()))?,
                    AddressId::parse_evm(network.clone(), &format!("{to:#x}"))?,
                    AssetId::native_eth(network.clone()),
                    None,
                    transaction.value(),
                    TransferKind::NativeExternal,
                )?));
            }
        }

        for (receipt_log_index, log) in receipt.logs().iter().enumerate() {
            ensure!(
                !log.removed,
                "removed log returned for finalized block {block_number}"
            );
            let log_index = to_u32(
                log.log_index.unwrap_or(receipt_log_index as u64),
                "log index",
            )?;
            log_rows.push(EvmLogRow {
                event_id: event_id(&config.eth_network_id, tx_hash, log_index),
                network_id: config.eth_network_id.clone(),
                block_hash: format!("{block_hash:#x}"),
                block_number,
                block_timestamp_unix_ms,
                tx_hash: format!("{tx_hash:#x}"),
                transaction_index,
                log_index,
                contract_address: format!("{:#x}", log.address()),
                topic0: log
                    .topic0()
                    .map(|topic| format!("{topic:#x}"))
                    .unwrap_or_default(),
                topics: log
                    .topics()
                    .iter()
                    .map(|topic| format!("{topic:#x}"))
                    .collect(),
                data: hex_bytes(log.inner.data.data.as_ref()),
            });

            for movement in token_movements(log) {
                let token_address = format!("{:#x}", movement.contract);
                discovered_tokens
                    .entry(token_address.clone())
                    .and_modify(|known| {
                        if movement.standard == TokenStandard::Erc1155 {
                            *known = TokenStandard::Erc1155;
                        }
                    })
                    .or_insert(movement.standard);
                if movement.amount.is_zero() {
                    continue;
                }

                let asset = token_asset(&network, movement.standard, &token_address)?;
                observed_movements.push(ObservedMovement {
                    log_index,
                    contract: movement.contract,
                    from: movement.from,
                    to: movement.to,
                    asset_id: asset.canonical_key(),
                    amount: movement.amount,
                });
                relationship_rows.push(relationship_row(CanonicalTransfer::new(
                    network.clone(),
                    block_hash,
                    block_number,
                    block_timestamp_unix_ms,
                    TransferOrigin::log_item(
                        tx_hash,
                        transaction_index,
                        log_index,
                        movement.event_sub_index,
                    ),
                    AddressId::parse_evm(network.clone(), &format!("{:#x}", movement.from))?,
                    AddressId::parse_evm(network.clone(), &format!("{:#x}", movement.to))?,
                    asset,
                    movement.token_id,
                    movement.amount,
                    movement_kind(movement.standard, movement.from, movement.to),
                )?));
            }
        }

        if receipt.status() {
            if let Some(trace) = traces_by_hash.get(&tx_hash) {
                ensure!(
                    trace.transaction_index == transaction_index,
                    "trace transaction index mismatch for {tx_hash:#x}"
                );
                append_internal_transfers(
                    &network,
                    block_hash,
                    block_number,
                    block_timestamp_unix_ms,
                    trace,
                    &mut relationship_rows,
                )?;
            }
        }

        let decoded = decode_transaction(
            semantic_registry,
            SemanticTransaction {
                network_id: &config.eth_network_id,
                block_number,
                block_timestamp_unix_ms,
                tx_hash,
                sender: transaction.from(),
                destination,
                input_selector: &input_selector,
                logs: receipt.logs(),
                movements: &observed_movements,
                relationships: &relationship_rows[relationship_start..],
            },
        );
        transaction_features.extend(decoded.transaction_features);
        semantic_events.extend(decoded.events);
    }

    ensure!(
        receipts_by_hash.is_empty(),
        "block {block_number} returned receipts that do not belong to its transactions"
    );

    if let Some(withdrawals) = &block.withdrawals {
        for withdrawal in withdrawals.iter() {
            let withdrawal_index = to_u32(withdrawal.index, "withdrawal index")?;
            relationship_rows.push(relationship_row(CanonicalTransfer::new(
                network.clone(),
                block_hash,
                block_number,
                block_timestamp_unix_ms,
                TransferOrigin::validator_withdrawal(withdrawal_index),
                AddressId::parse_evm(network.clone(), &format!("{:#x}", Address::ZERO))?,
                AddressId::parse_evm(network.clone(), &format!("{:#x}", withdrawal.address))?,
                AssetId::native_eth(network.clone()),
                None,
                withdrawal.amount_wei(),
                TransferKind::ValidatorWithdrawal,
            )?));
        }
    }

    let token_discoveries = discovered_tokens
        .iter()
        .map(|(token_address, standard)| TokenMetadataDiscoveryRow {
            network_id: config.eth_network_id.clone(),
            token_address: token_address.clone(),
            token_standard: standard.as_str().to_string(),
            discovered_block: block_number,
            discovered_at_unix_ms: block_timestamp_unix_ms,
        })
        .collect();
    let token_jobs = discovered_tokens
        .iter()
        .map(|(token_address, standard)| TokenMetadataJobRow {
            network_id: config.eth_network_id.clone(),
            token_address: token_address.clone(),
            token_standard: standard.as_str().to_string(),
            discovered_block: block_number,
            status: "pending".to_string(),
            attempt_count: 0,
            last_error: String::new(),
            updated_at_unix_ms: indexed_at_unix_ms,
        })
        .collect();

    Ok(ExtractedBlock {
        block: IngestedBlockRow {
            network_id: config.eth_network_id.clone(),
            block_number,
            block_hash: format!("{block_hash:#x}"),
            parent_hash: format!("{:#x}", block.header.parent_hash),
            block_timestamp_unix_ms,
            transaction_count: to_u32(transactions.len() as u64, "transaction count")?,
            ingestion_status: "complete".to_string(),
            receipt_data_complete: 1,
            trace_data_complete: u8::from(traces.is_some()),
            rpc_provider: config.eth_rpc_provider.clone(),
            error_message: String::new(),
            indexed_at_unix_ms,
        },
        transactions: transaction_rows,
        logs: log_rows,
        relationships: relationship_rows,
        transaction_features,
        semantic_events,
        token_discoveries,
        token_jobs,
    })
}

fn append_internal_transfers(
    network: &NetworkId,
    block_hash: B256,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    trace: &TransactionTrace,
    relationships: &mut Vec<AddressRelationshipRow>,
) -> anyhow::Result<()> {
    for (index, child) in trace.root.calls.iter().enumerate() {
        append_call_frame(
            network,
            block_hash,
            block_number,
            block_timestamp_unix_ms,
            trace,
            child,
            vec![to_u32(index as u64, "trace index")?],
            relationships,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_call_frame(
    network: &NetworkId,
    block_hash: B256,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    trace: &TransactionTrace,
    frame: &alloy::rpc::types::trace::geth::CallFrame,
    trace_address: Vec<u32>,
    relationships: &mut Vec<AddressRelationshipRow>,
) -> anyhow::Result<()> {
    if frame.error.is_some() || frame.revert_reason.is_some() {
        return Ok(());
    }

    let call_type = frame.typ.to_ascii_uppercase();
    let carries_value = matches!(
        call_type.as_str(),
        "CALL" | "CREATE" | "CREATE2" | "SELFDESTRUCT"
    );
    if carries_value {
        if let (Some(to), Some(value)) = (frame.to, frame.value) {
            if !value.is_zero() && frame.from != to {
                relationships.push(relationship_row(CanonicalTransfer::new(
                    network.clone(),
                    block_hash,
                    block_number,
                    block_timestamp_unix_ms,
                    TransferOrigin::trace(
                        trace.tx_hash,
                        trace.transaction_index,
                        trace_address.clone(),
                    ),
                    AddressId::parse_evm(network.clone(), &format!("{:#x}", frame.from))?,
                    AddressId::parse_evm(network.clone(), &format!("{to:#x}"))?,
                    AssetId::native_eth(network.clone()),
                    None,
                    value,
                    TransferKind::NativeInternal,
                )?));
            }
        }
    }

    for (index, child) in frame.calls.iter().enumerate() {
        let mut child_address = trace_address.clone();
        child_address.push(to_u32(index as u64, "trace index")?);
        append_call_frame(
            network,
            block_hash,
            block_number,
            block_timestamp_unix_ms,
            trace,
            child,
            child_address,
            relationships,
        )?;
    }
    Ok(())
}

fn validate_receipt(
    block_number: u64,
    block_hash: B256,
    transaction_index: Option<u64>,
    receipt: &TransactionReceipt,
) -> anyhow::Result<()> {
    ensure!(
        receipt.block_number == Some(block_number),
        "receipt {} belongs to a different block number",
        receipt.transaction_hash
    );
    ensure!(
        receipt.block_hash == Some(block_hash),
        "receipt {} belongs to a different block hash",
        receipt.transaction_hash
    );
    ensure!(
        receipt.transaction_index == transaction_index,
        "receipt {} transaction index mismatch",
        receipt.transaction_hash
    );
    Ok(())
}

fn token_movements(log: &Log) -> Vec<TokenMovement> {
    let topics = log.topics();
    match topics.first() {
        Some(topic0) if topic0 == &*ERC_TRANSFER_EVENT => {
            decode_erc20_or_erc721(log).into_iter().collect()
        }
        Some(topic0) if topic0 == &*ERC1155_TRANSFER_SINGLE_EVENT => {
            decode_erc1155_single(log).into_iter().collect()
        }
        Some(topic0) if topic0 == &*ERC1155_TRANSFER_BATCH_EVENT => decode_erc1155_batch(log),
        _ => Vec::new(),
    }
}

fn decode_erc20_or_erc721(log: &Log) -> Option<TokenMovement> {
    let topics = log.topics();
    let from = topic_address(*topics.get(1)?)?;
    let to = topic_address(*topics.get(2)?)?;
    match topics.len() {
        3 if log.inner.data.data.len() == 32 => Some(TokenMovement {
            contract: log.address(),
            from,
            to,
            amount: U256::from_be_slice(log.inner.data.data.as_ref()),
            token_id: None,
            standard: TokenStandard::Erc20,
            event_sub_index: 0,
        }),
        4 if log.inner.data.data.is_empty() => Some(TokenMovement {
            contract: log.address(),
            from,
            to,
            amount: U256::from(1_u8),
            token_id: Some(U256::from_be_slice(topics[3].as_slice())),
            standard: TokenStandard::Erc721,
            event_sub_index: 0,
        }),
        _ => None,
    }
}

fn decode_erc1155_single(log: &Log) -> Option<TokenMovement> {
    let topics = log.topics();
    if topics.len() != 4 || log.inner.data.data.len() != 64 {
        return None;
    }
    Some(TokenMovement {
        contract: log.address(),
        from: topic_address(*topics.get(2)?)?,
        to: topic_address(*topics.get(3)?)?,
        token_id: Some(U256::from_be_slice(word(log.inner.data.data.as_ref(), 0)?)),
        amount: U256::from_be_slice(word(log.inner.data.data.as_ref(), 1)?),
        standard: TokenStandard::Erc1155,
        event_sub_index: 0,
    })
}

fn decode_erc1155_batch(log: &Log) -> Vec<TokenMovement> {
    let topics = log.topics();
    if topics.len() != 4 {
        return Vec::new();
    }
    let Some(from) = topics.get(2).copied().and_then(topic_address) else {
        return Vec::new();
    };
    let Some(to) = topics.get(3).copied().and_then(topic_address) else {
        return Vec::new();
    };
    let data = log.inner.data.data.as_ref();
    let Some(ids_offset) = word(data, 0).and_then(word_usize) else {
        return Vec::new();
    };
    let Some(values_offset) = word(data, 1).and_then(word_usize) else {
        return Vec::new();
    };
    let Some(ids) = abi_u256_array(data, ids_offset) else {
        return Vec::new();
    };
    let Some(values) = abi_u256_array(data, values_offset) else {
        return Vec::new();
    };
    if ids.len() != values.len() {
        return Vec::new();
    }

    ids.into_iter()
        .zip(values)
        .enumerate()
        .filter_map(|(index, (token_id, amount))| {
            Some(TokenMovement {
                contract: log.address(),
                from,
                to,
                amount,
                token_id: Some(token_id),
                standard: TokenStandard::Erc1155,
                event_sub_index: u32::try_from(index).ok()?,
            })
        })
        .collect()
}

fn abi_u256_array(data: &[u8], offset: usize) -> Option<Vec<U256>> {
    if offset % 32 != 0 {
        return None;
    }
    let length = data
        .get(offset..offset.checked_add(32)?)
        .and_then(word_usize)?;
    let items_start = offset.checked_add(32)?;
    let items_end = items_start.checked_add(length.checked_mul(32)?)?;
    let items = data.get(items_start..items_end)?;
    Some(items.chunks_exact(32).map(U256::from_be_slice).collect())
}

fn word(data: &[u8], index: usize) -> Option<&[u8]> {
    let start = index.checked_mul(32)?;
    data.get(start..start.checked_add(32)?)
}

fn word_usize(word: &[u8]) -> Option<usize> {
    if word.len() != 32 || word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    let mut tail = [0_u8; 8];
    tail.copy_from_slice(&word[24..]);
    usize::try_from(u64::from_be_bytes(tail)).ok()
}

fn token_asset(
    network: &NetworkId,
    standard: TokenStandard,
    token_address: &str,
) -> anyhow::Result<AssetId> {
    Ok(match standard {
        TokenStandard::Erc20 => AssetId::erc20(network.clone(), token_address)?,
        TokenStandard::Erc721 => AssetId::erc721(network.clone(), token_address)?,
        TokenStandard::Erc1155 => AssetId::erc1155(network.clone(), token_address)?,
    })
}

fn movement_kind(standard: TokenStandard, from: Address, to: Address) -> TransferKind {
    match (standard, from == Address::ZERO, to == Address::ZERO) {
        (TokenStandard::Erc20, true, _) => TransferKind::Erc20Mint,
        (TokenStandard::Erc20, _, true) => TransferKind::Erc20Burn,
        (TokenStandard::Erc20, _, _) => TransferKind::Erc20,
        (TokenStandard::Erc721, true, _) => TransferKind::Erc721Mint,
        (TokenStandard::Erc721, _, true) => TransferKind::Erc721Burn,
        (TokenStandard::Erc721, _, _) => TransferKind::Erc721,
        (TokenStandard::Erc1155, true, _) => TransferKind::Erc1155Mint,
        (TokenStandard::Erc1155, _, true) => TransferKind::Erc1155Burn,
        (TokenStandard::Erc1155, _, _) => TransferKind::Erc1155,
    }
}

fn relationship_row(transfer: CanonicalTransfer) -> AddressRelationshipRow {
    AddressRelationshipRow {
        relationship_id: transfer.relationship_id,
        network_id: transfer.network.to_string(),
        block_hash: format!("{:#x}", transfer.block_hash),
        block_number: transfer.block_number,
        block_timestamp_unix_ms: transfer.block_timestamp_unix_ms,
        tx_hash: transfer
            .origin
            .tx_hash
            .map(|hash| format!("{hash:#x}"))
            .unwrap_or_default(),
        transaction_index: transfer.origin.transaction_index,
        event_index: transfer.origin.event_index,
        event_sub_index: transfer.origin.event_sub_index,
        trace_address: transfer.origin.trace_address,
        from_address: transfer.from.address().to_string(),
        to_address: transfer.to.address().to_string(),
        asset_id: transfer.asset.canonical_key(),
        token_id: transfer
            .token_id
            .map(|token_id| token_id.to_string())
            .unwrap_or_default(),
        amount: clickhouse_u256(transfer.amount),
        transfer_type: transfer.kind.as_str().to_string(),
    }
}

fn topic_address(topic: B256) -> Option<Address> {
    let bytes = topic.as_slice();
    (bytes[..12].iter().all(|byte| *byte == 0)).then(|| Address::from_slice(&bytes[12..]))
}

fn clickhouse_u256(value: U256) -> UInt256 {
    UInt256::from_le_bytes(value.to_le_bytes())
}

fn event_id(network_id: &str, tx_hash: B256, log_index: u32) -> String {
    let identity = format!("{network_id}|{tx_hash:#x}|{log_index}");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn hex_bytes(bytes: &[u8]) -> String {
    format!("0x{}", alloy::hex::encode(bytes))
}

fn to_u32(value: u64, field: &str) -> anyhow::Result<u32> {
    u32::try_from(value).with_context(|| format!("{field} {value} exceeds UInt32"))
}

#[cfg(test)]
mod tests {
    use alloy::{
        primitives::{Address, Bytes, Log as PrimitiveLog, U256, address},
        rpc::types::Log,
    };

    use super::{
        ERC_TRANSFER_EVENT, ERC1155_TRANSFER_BATCH_EVENT, ERC1155_TRANSFER_SINGLE_EVENT,
        TokenStandard, movement_kind, token_movements,
    };
    use crate::domain::TransferKind;

    fn address_topic(address: Address) -> alloy::primitives::B256 {
        let mut bytes = [0_u8; 32];
        bytes[12..].copy_from_slice(address.as_slice());
        bytes.into()
    }

    fn encoded_word(value: U256) -> [u8; 32] {
        value.to_be_bytes()
    }

    #[test]
    fn decodes_erc20_transfer_log() {
        let contract = address!("0000000000000000000000000000000000000010");
        let from = address!("0000000000000000000000000000000000000020");
        let to = address!("0000000000000000000000000000000000000030");
        let amount = U256::from(42_u64);
        let log = Log {
            inner: PrimitiveLog::new_unchecked(
                contract,
                vec![*ERC_TRANSFER_EVENT, address_topic(from), address_topic(to)],
                Bytes::copy_from_slice(&encoded_word(amount)),
            ),
            ..Default::default()
        };

        let movement = token_movements(&log).pop().expect("valid ERC-20 transfer");
        assert_eq!(movement.standard, TokenStandard::Erc20);
        assert_eq!(movement.amount, amount);
    }

    #[test]
    fn decodes_erc1155_single() {
        let contract = address!("0000000000000000000000000000000000000010");
        let operator = address!("0000000000000000000000000000000000000011");
        let from = address!("0000000000000000000000000000000000000020");
        let to = address!("0000000000000000000000000000000000000030");
        let mut data = Vec::new();
        data.extend_from_slice(&encoded_word(U256::from(7)));
        data.extend_from_slice(&encoded_word(U256::from(3)));
        let log = Log {
            inner: PrimitiveLog::new_unchecked(
                contract,
                vec![
                    *ERC1155_TRANSFER_SINGLE_EVENT,
                    address_topic(operator),
                    address_topic(from),
                    address_topic(to),
                ],
                Bytes::from(data),
            ),
            ..Default::default()
        };

        let movement = token_movements(&log).pop().expect("valid ERC-1155 single");
        assert_eq!(movement.standard, TokenStandard::Erc1155);
        assert_eq!(movement.token_id, Some(U256::from(7)));
        assert_eq!(movement.amount, U256::from(3));
    }

    #[test]
    fn decodes_erc1155_batch_with_distinct_sub_indexes() {
        let contract = address!("0000000000000000000000000000000000000010");
        let operator = address!("0000000000000000000000000000000000000011");
        let from = address!("0000000000000000000000000000000000000020");
        let to = address!("0000000000000000000000000000000000000030");
        let mut data = Vec::new();
        for value in [64_u64, 160, 2, 7, 8, 2, 3, 4] {
            data.extend_from_slice(&encoded_word(U256::from(value)));
        }
        let log = Log {
            inner: PrimitiveLog::new_unchecked(
                contract,
                vec![
                    *ERC1155_TRANSFER_BATCH_EVENT,
                    address_topic(operator),
                    address_topic(from),
                    address_topic(to),
                ],
                Bytes::from(data),
            ),
            ..Default::default()
        };

        let movements = token_movements(&log);
        assert_eq!(movements.len(), 2);
        assert_eq!(movements[0].event_sub_index, 0);
        assert_eq!(movements[1].event_sub_index, 1);
        assert_eq!(movements[1].token_id, Some(U256::from(8)));
        assert_eq!(movements[1].amount, U256::from(4));
    }

    #[test]
    fn classifies_erc1155_mint_and_burn() {
        let wallet = address!("0000000000000000000000000000000000000001");
        assert_eq!(
            movement_kind(TokenStandard::Erc1155, Address::ZERO, wallet),
            TransferKind::Erc1155Mint
        );
        assert_eq!(
            movement_kind(TokenStandard::Erc1155, wallet, Address::ZERO),
            TransferKind::Erc1155Burn
        );
    }
}
