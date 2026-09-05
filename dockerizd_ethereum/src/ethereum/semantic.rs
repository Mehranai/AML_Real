use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::LazyLock,
};

use alloy::{
    primitives::{Address, B256, U256, keccak256},
    rpc::types::Log,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::storage::{
    AddressRelationshipRow, ProtocolContractRow, SemanticAmlEventRow, TransactionFeatureRow,
};

const DETECTOR: &str = "ethereum_semantic_decoder";
const DETECTOR_VERSION: &str = "ethereum_semantic_v2";

static SWAP_V2_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Swap(address,uint256,uint256,uint256,uint256,address)"));
static SWAP_V3_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Swap(address,address,int256,int256,uint160,uint128,int24)"));
static CURVE_SWAP_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("TokenExchange(address,int128,uint256,int128,uint256)"));
static CURVE_UNDERLYING_SWAP_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("TokenExchangeUnderlying(address,int128,uint256,int128,uint256)"));
static BALANCER_SWAP_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Swap(bytes32,address,address,uint256,uint256)"));

static UNISWAP_V2_MINT_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Mint(address,uint256,uint256)"));
static UNISWAP_V2_BURN_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Burn(address,uint256,uint256,address)"));
static UNISWAP_V3_MINT_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Mint(address,address,int24,int24,uint128,uint256,uint256)"));
static UNISWAP_V3_BURN_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Burn(address,int24,int24,uint128,uint256,uint256)"));
static TORNADO_DEPOSIT_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Deposit(bytes32,uint32,uint256)"));
static TORNADO_WITHDRAWAL_EVENT: LazyLock<B256> =
    LazyLock::new(|| signature("Withdrawal(address,bytes32,address,uint256)"));

static OP_ERC20_BRIDGE_INITIATED: LazyLock<B256> = LazyLock::new(|| {
    signature("ERC20BridgeInitiated(address,address,address,address,uint256,bytes)")
});
static OP_ERC20_BRIDGE_FINALIZED: LazyLock<B256> = LazyLock::new(|| {
    signature("ERC20BridgeFinalized(address,address,address,address,uint256,bytes)")
});
static OP_ETH_BRIDGE_INITIATED: LazyLock<B256> =
    LazyLock::new(|| signature("ETHBridgeInitiated(address,address,uint256,bytes)"));
static OP_ETH_BRIDGE_FINALIZED: LazyLock<B256> =
    LazyLock::new(|| signature("ETHBridgeFinalized(address,address,uint256,bytes)"));
static OP_ERC20_DEPOSIT_INITIATED: LazyLock<B256> = LazyLock::new(|| {
    signature("ERC20DepositInitiated(address,address,address,address,uint256,bytes)")
});
static OP_ETH_DEPOSIT_INITIATED: LazyLock<B256> =
    LazyLock::new(|| signature("ETHDepositInitiated(address,address,uint256,bytes)"));
static OP_ERC20_WITHDRAWAL_FINALIZED: LazyLock<B256> = LazyLock::new(|| {
    signature("ERC20WithdrawalFinalized(address,address,address,address,uint256,bytes)")
});
static OP_ETH_WITHDRAWAL_FINALIZED: LazyLock<B256> =
    LazyLock::new(|| signature("ETHWithdrawalFinalized(address,address,uint256,bytes)"));

#[derive(Debug, Clone)]
pub struct ObservedMovement {
    pub log_index: u32,
    pub contract: Address,
    pub from: Address,
    pub to: Address,
    pub asset_id: String,
    pub amount: U256,
}

pub struct SemanticTransaction<'a> {
    pub network_id: &'a str,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub tx_hash: B256,
    pub sender: Address,
    pub destination: Option<Address>,
    pub input_selector: &'a str,
    pub logs: &'a [Log],
    pub movements: &'a [ObservedMovement],
    pub relationships: &'a [AddressRelationshipRow],
}

#[derive(Debug, Default)]
pub struct SemanticDecodeResult {
    pub transaction_features: Vec<TransactionFeatureRow>,
    pub events: Vec<SemanticAmlEventRow>,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticDecoderRegistry {
    contracts: HashMap<Address, ProtocolContractRow>,
}

impl SemanticDecoderRegistry {
    pub fn from_contracts(rows: Vec<ProtocolContractRow>) -> Self {
        let contracts = rows
            .into_iter()
            .filter(|row| row.enabled == 1)
            .filter_map(|row| {
                Address::from_str(&row.contract_address)
                    .ok()
                    .map(|address| (address, row))
            })
            .collect();
        Self { contracts }
    }

    fn contract(&self, address: Address) -> Option<&ProtocolContractRow> {
        self.contracts.get(&address)
    }
}

pub fn decode_transaction(
    registry: &SemanticDecoderRegistry,
    transaction: SemanticTransaction<'_>,
) -> SemanticDecodeResult {
    let mut events = Vec::new();

    for (fallback_index, log) in transaction.logs.iter().enumerate() {
        let Some(topic0) = log.topic0() else {
            continue;
        };
        let log_index = log
            .log_index
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(fallback_index as u32);

        if let Some(contract) = registry.contract(log.address()) {
            if contract.protocol_type == "bridge" && contract.decoder == "op_standard_bridge_v1" {
                if let Some(event) = decode_op_bridge(&transaction, log, log_index, contract) {
                    events.push(event);
                    continue;
                }
            }
            if contract.protocol_type == "mixer" && contract.decoder == "tornado_cash_v1" {
                if let Some(event) = decode_tornado_mixer(&transaction, log, log_index, contract) {
                    events.push(event);
                    continue;
                }
            }
        }

        if let Some((event_type, family)) = liquidity_family(topic0) {
            events.push(decode_liquidity(
                registry,
                &transaction,
                log,
                log_index,
                event_type,
                family,
            ));
            continue;
        }

        if let Some(family) = swap_family(topic0) {
            events.push(decode_swap(registry, &transaction, log, log_index, family));
        }
    }

    let transaction_features = (!events.is_empty())
        .then(|| transaction_feature(&transaction, &events))
        .into_iter()
        .collect();

    SemanticDecodeResult {
        transaction_features,
        events,
    }
}

fn decode_swap(
    registry: &SemanticDecoderRegistry,
    transaction: &SemanticTransaction<'_>,
    log: &Log,
    log_index: u32,
    family: &'static str,
) -> SemanticAmlEventRow {
    let pool = log.address();
    let registered = registry
        .contract(pool)
        .filter(|contract| contract.protocol_type == "dex");
    let protocol = registered
        .map(|contract| contract.protocol.clone())
        .unwrap_or_else(|| family.to_string());

    let direct_balancer = (family == "balancer_vault")
        .then(|| decode_balancer_assets(transaction.network_id, log))
        .flatten();
    let inferred = infer_pool_flows(transaction.movements, pool);
    let (asset_in, amount_in, asset_out, amount_out, flow_confidence) = direct_balancer
        .map(|(asset_in, amount_in, asset_out, amount_out)| {
            (asset_in, amount_in, asset_out, amount_out, 0.95_f32)
        })
        .or_else(|| {
            inferred.map(|(asset_in, amount_in, asset_out, amount_out)| {
                (asset_in, amount_in, asset_out, amount_out, 0.86_f32)
            })
        })
        .unwrap_or_else(|| {
            (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                0.72_f32,
            )
        });
    let confidence = registered
        .map(|contract| contract.confidence.max(flow_confidence))
        .unwrap_or(flow_confidence);
    let event_id = semantic_event_id(
        transaction.network_id,
        transaction.tx_hash,
        log_index,
        "swap",
        pool,
    );
    let related_movements = transaction
        .movements
        .iter()
        .filter(|movement| movement.from == pool || movement.to == pool)
        .map(|movement| {
            json!({
                "log_index": movement.log_index,
                "asset_id": movement.asset_id,
                "from": format!("{:#x}", movement.from),
                "to": format!("{:#x}", movement.to),
                "amount": movement.amount.to_string(),
                "token_contract": format!("{:#x}", movement.contract),
            })
        })
        .collect::<Vec<_>>();
    let evidence = json!({
        "event_signature": format!("{topic:#x}", topic = log.topic0().unwrap_or_default()),
        "event_family": family,
        "emitter": format!("{pool:#x}"),
        "log_index": log_index,
        "flow_correlation": related_movements,
        "registered_protocol": registered.is_some(),
    });

    SemanticAmlEventRow {
        event_id,
        network_id: transaction.network_id.to_string(),
        tx_hash: format!("{:#x}", transaction.tx_hash),
        block_number: transaction.block_number,
        block_timestamp_unix_ms: transaction.block_timestamp_unix_ms,
        event_index: log_index,
        event_type: "swap".to_string(),
        subject_address: format!("{:#x}", transaction.sender),
        protocol,
        protocol_contract: format!("{pool:#x}"),
        counterparty_address: format!("{pool:#x}"),
        asset_in,
        asset_out,
        remote_network_id: String::new(),
        remote_asset: String::new(),
        bridge_direction: String::new(),
        correlation_key: String::new(),
        amount_in,
        amount_out,
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        confidence,
        evidence_json: evidence.to_string(),
    }
}

fn decode_op_bridge(
    transaction: &SemanticTransaction<'_>,
    log: &Log,
    log_index: u32,
    contract: &ProtocolContractRow,
) -> Option<SemanticAmlEventRow> {
    let topic0 = log.topic0()?;
    let direction = if is_any(
        topic0,
        &[
            &OP_ERC20_BRIDGE_INITIATED,
            &OP_ETH_BRIDGE_INITIATED,
            &OP_ERC20_DEPOSIT_INITIATED,
            &OP_ETH_DEPOSIT_INITIATED,
        ],
    ) {
        "outbound"
    } else if is_any(
        topic0,
        &[
            &OP_ERC20_BRIDGE_FINALIZED,
            &OP_ETH_BRIDGE_FINALIZED,
            &OP_ERC20_WITHDRAWAL_FINALIZED,
            &OP_ETH_WITHDRAWAL_FINALIZED,
        ],
    ) {
        "inbound"
    } else {
        return None;
    };

    let is_erc20 = is_any(
        topic0,
        &[
            &OP_ERC20_BRIDGE_INITIATED,
            &OP_ERC20_BRIDGE_FINALIZED,
            &OP_ERC20_DEPOSIT_INITIATED,
            &OP_ERC20_WITHDRAWAL_FINALIZED,
        ],
    );
    let (from, to, local_asset, remote_asset, amount) = if is_erc20 {
        let topics = log.topics();
        let local_token = topic_address(*topics.get(1)?)?;
        let remote_token = topic_address(*topics.get(2)?)?;
        let from = topic_address(*topics.get(3)?)?;
        let to = word_address(word(log, 0)?)?;
        let amount = U256::from_be_slice(word(log, 1)?);
        (
            from,
            to,
            evm_asset(transaction.network_id, "erc20", local_token),
            evm_asset(&contract.remote_network_id, "erc20", remote_token),
            amount,
        )
    } else {
        let topics = log.topics();
        let from = topic_address(*topics.get(1)?)?;
        let to = topic_address(*topics.get(2)?)?;
        let amount = U256::from_be_slice(word(log, 0)?);
        (
            from,
            to,
            format!("{}/native:eth", transaction.network_id),
            format!("{}/native:eth", contract.remote_network_id),
            amount,
        )
    };
    if amount.is_zero() {
        return None;
    }

    let subject = if direction == "outbound" { from } else { to };
    let counterparty = if direction == "outbound" { to } else { from };
    let correlation_key = bridge_correlation_key(
        &contract.protocol,
        transaction.network_id,
        &contract.remote_network_id,
        &local_asset,
        &remote_asset,
        from,
        to,
        amount,
    );
    let event_id = semantic_event_id(
        transaction.network_id,
        transaction.tx_hash,
        log_index,
        "bridge_transfer",
        log.address(),
    );
    let evidence = json!({
        "event_signature": format!("{topic0:#x}"),
        "emitter": format!("{:#x}", log.address()),
        "log_index": log_index,
        "from": format!("{from:#x}"),
        "to": format!("{to:#x}"),
        "amount": amount.to_string(),
        "registry_source": contract.source,
        "contract_role": contract.contract_role,
        "remote_contract": contract.remote_contract_address,
        "correlation_key_is_probabilistic": true,
    });

    Some(SemanticAmlEventRow {
        event_id,
        network_id: transaction.network_id.to_string(),
        tx_hash: format!("{:#x}", transaction.tx_hash),
        block_number: transaction.block_number,
        block_timestamp_unix_ms: transaction.block_timestamp_unix_ms,
        event_index: log_index,
        event_type: "bridge_transfer".to_string(),
        subject_address: format!("{subject:#x}"),
        protocol: contract.protocol.clone(),
        protocol_contract: format!("{:#x}", log.address()),
        counterparty_address: format!("{counterparty:#x}"),
        asset_in: local_asset,
        asset_out: String::new(),
        remote_network_id: contract.remote_network_id.clone(),
        remote_asset,
        bridge_direction: direction.to_string(),
        correlation_key,
        amount_in: (direction == "outbound")
            .then(|| amount.to_string())
            .unwrap_or_default(),
        amount_out: (direction == "inbound")
            .then(|| amount.to_string())
            .unwrap_or_default(),
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        confidence: contract.confidence,
        evidence_json: evidence.to_string(),
    })
}

fn liquidity_family(topic0: &B256) -> Option<(&'static str, &'static str)> {
    if topic0 == &*UNISWAP_V2_MINT_EVENT {
        Some(("liquidity_add", "uniswap_v2_compatible"))
    } else if topic0 == &*UNISWAP_V2_BURN_EVENT {
        Some(("liquidity_remove", "uniswap_v2_compatible"))
    } else if topic0 == &*UNISWAP_V3_MINT_EVENT {
        Some(("liquidity_add", "uniswap_v3_compatible"))
    } else if topic0 == &*UNISWAP_V3_BURN_EVENT {
        Some(("liquidity_remove", "uniswap_v3_compatible"))
    } else {
        None
    }
}

fn decode_liquidity(
    registry: &SemanticDecoderRegistry,
    transaction: &SemanticTransaction<'_>,
    log: &Log,
    log_index: u32,
    event_type: &'static str,
    family: &'static str,
) -> SemanticAmlEventRow {
    let pool = log.address();
    let registered = registry
        .contract(pool)
        .filter(|contract| contract.protocol_type == "dex");
    let protocol = registered
        .map(|contract| contract.protocol.clone())
        .unwrap_or_else(|| family.to_string());
    let inbound = event_type == "liquidity_add";
    let (assets, amounts, flow_count) =
        directional_relationship_flows(transaction.relationships, pool, inbound);
    let flow_confidence = if flow_count > 0 { 0.88_f32 } else { 0.72_f32 };
    let confidence = registered
        .map(|contract| contract.confidence.max(flow_confidence))
        .unwrap_or(flow_confidence);
    let evidence = json!({
        "event_signature": format!("{:#x}", log.topic0().unwrap_or_default()),
        "event_family": family,
        "emitter": format!("{pool:#x}"),
        "log_index": log_index,
        "flow_direction": if inbound { "into_pool" } else { "out_of_pool" },
        "flow_count": flow_count,
        "registered_protocol": registered.is_some(),
    });

    SemanticAmlEventRow {
        event_id: semantic_event_id(
            transaction.network_id,
            transaction.tx_hash,
            log_index,
            event_type,
            pool,
        ),
        network_id: transaction.network_id.to_string(),
        tx_hash: format!("{:#x}", transaction.tx_hash),
        block_number: transaction.block_number,
        block_timestamp_unix_ms: transaction.block_timestamp_unix_ms,
        event_index: log_index,
        event_type: event_type.to_string(),
        subject_address: format!("{:#x}", transaction.sender),
        protocol,
        protocol_contract: format!("{pool:#x}"),
        counterparty_address: format!("{pool:#x}"),
        asset_in: if inbound {
            assets.clone()
        } else {
            String::new()
        },
        asset_out: if inbound {
            String::new()
        } else {
            assets.clone()
        },
        remote_network_id: String::new(),
        remote_asset: String::new(),
        bridge_direction: String::new(),
        correlation_key: String::new(),
        amount_in: if inbound {
            amounts.clone()
        } else {
            String::new()
        },
        amount_out: if inbound { String::new() } else { amounts },
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        confidence,
        evidence_json: evidence.to_string(),
    }
}

fn decode_tornado_mixer(
    transaction: &SemanticTransaction<'_>,
    log: &Log,
    log_index: u32,
    contract: &ProtocolContractRow,
) -> Option<SemanticAmlEventRow> {
    let topic0 = log.topic0()?;
    let (event_type, inbound, subject) = if topic0 == &*TORNADO_DEPOSIT_EVENT {
        ("mixer_deposit", true, transaction.sender)
    } else if topic0 == &*TORNADO_WITHDRAWAL_EVENT {
        (
            "mixer_withdrawal",
            false,
            log.topics()
                .get(1)
                .copied()
                .and_then(topic_address)
                .unwrap_or(transaction.sender),
        )
    } else {
        return None;
    };
    let pool = log.address();
    let (assets, amounts, flow_count) =
        directional_relationship_flows(transaction.relationships, pool, inbound);
    let evidence = json!({
        "event_signature": format!("{topic0:#x}"),
        "emitter": format!("{pool:#x}"),
        "log_index": log_index,
        "topics": log.topics().iter().map(|topic| format!("{topic:#x}")).collect::<Vec<_>>(),
        "flow_direction": if inbound { "into_mixer" } else { "out_of_mixer" },
        "flow_count": flow_count,
        "registry_source": contract.source,
        "contract_role": contract.contract_role,
    });
    let correlation_material = log
        .topics()
        .get(1)
        .map(|topic| format!("{topic:#x}"))
        .unwrap_or_else(|| format!("{:#x}", transaction.tx_hash));

    Some(SemanticAmlEventRow {
        event_id: semantic_event_id(
            transaction.network_id,
            transaction.tx_hash,
            log_index,
            event_type,
            pool,
        ),
        network_id: transaction.network_id.to_string(),
        tx_hash: format!("{:#x}", transaction.tx_hash),
        block_number: transaction.block_number,
        block_timestamp_unix_ms: transaction.block_timestamp_unix_ms,
        event_index: log_index,
        event_type: event_type.to_string(),
        subject_address: format!("{subject:#x}"),
        protocol: contract.protocol.clone(),
        protocol_contract: format!("{pool:#x}"),
        counterparty_address: format!("{pool:#x}"),
        asset_in: if inbound {
            assets.clone()
        } else {
            String::new()
        },
        asset_out: if inbound {
            String::new()
        } else {
            assets.clone()
        },
        remote_network_id: String::new(),
        remote_asset: String::new(),
        bridge_direction: String::new(),
        correlation_key: stable_hash(&format!(
            "{}|{}|{}",
            contract.protocol, event_type, correlation_material
        )),
        amount_in: if inbound {
            amounts.clone()
        } else {
            String::new()
        },
        amount_out: if inbound { String::new() } else { amounts },
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        confidence: if flow_count > 0 {
            contract.confidence
        } else {
            contract.confidence.min(0.82)
        },
        evidence_json: evidence.to_string(),
    })
}

fn directional_relationship_flows(
    relationships: &[AddressRelationshipRow],
    pool: Address,
    inbound: bool,
) -> (String, String, usize) {
    let pool = format!("{pool:#x}");
    let mut totals = HashMap::<String, U256>::new();

    for relationship in relationships {
        let matches_direction = if inbound {
            relationship.to_address == pool && relationship.from_address != pool
        } else {
            relationship.from_address == pool && relationship.to_address != pool
        };
        if !matches_direction {
            continue;
        }
        let amount = U256::from_le_bytes(relationship.amount.to_le_bytes());
        let total = totals.entry(relationship.asset_id.clone()).or_default();
        *total = total.checked_add(amount).unwrap_or(U256::MAX);
    }

    let mut totals = totals.into_iter().collect::<Vec<_>>();
    totals.sort_by(|left, right| left.0.cmp(&right.0));
    let flow_count = totals.len();
    let assets = totals
        .iter()
        .map(|(asset, _)| asset.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let amounts = totals
        .iter()
        .map(|(_, amount)| amount.to_string())
        .collect::<Vec<_>>()
        .join(",");
    (assets, amounts, flow_count)
}

fn transaction_feature(
    transaction: &SemanticTransaction<'_>,
    events: &[SemanticAmlEventRow],
) -> TransactionFeatureRow {
    let assets = transaction
        .relationships
        .iter()
        .map(|relationship| relationship.asset_id.as_str())
        .collect::<HashSet<_>>();
    let participants = transaction
        .relationships
        .iter()
        .flat_map(|relationship| {
            [
                relationship.from_address.as_str(),
                relationship.to_address.as_str(),
            ]
        })
        .collect::<HashSet<_>>();
    let protocols = events
        .iter()
        .map(|event| event.protocol.as_str())
        .collect::<HashSet<_>>();
    let event_types = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<HashSet<_>>();
    let evidence_refs = events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<Vec<_>>();
    let is_swap = u8::from(events.iter().any(|event| event.event_type == "swap"));
    let is_bridge = u8::from(
        events
            .iter()
            .any(|event| event.event_type == "bridge_transfer"),
    );
    let is_mixer = u8::from(
        events
            .iter()
            .any(|event| event.event_type.starts_with("mixer_")),
    );
    let is_liquidity_add = u8::from(
        events
            .iter()
            .any(|event| event.event_type == "liquidity_add"),
    );
    let is_liquidity_remove = u8::from(
        events
            .iter()
            .any(|event| event.event_type == "liquidity_remove"),
    );

    TransactionFeatureRow {
        feature_id: stable_hash(&format!(
            "{}|{:#x}|{}",
            transaction.network_id, transaction.tx_hash, DETECTOR_VERSION
        )),
        network_id: transaction.network_id.to_string(),
        tx_hash: format!("{:#x}", transaction.tx_hash),
        block_number: transaction.block_number,
        block_timestamp_unix_ms: transaction.block_timestamp_unix_ms,
        transaction_type: if is_mixer == 1 {
            "mixer".to_string()
        } else if is_bridge == 1 {
            "bridge".to_string()
        } else if is_liquidity_add == 1 || is_liquidity_remove == 1 {
            "liquidity".to_string()
        } else if is_swap == 1 {
            "swap".to_string()
        } else {
            "contract_call".to_string()
        },
        transaction_subtype: sorted_join(event_types),
        protocol: sorted_join(protocols),
        method_id: transaction.input_selector.to_string(),
        is_swap,
        is_bridge,
        is_mixer,
        is_mint: u8::from(
            transaction
                .relationships
                .iter()
                .any(|relationship| relationship.transfer_type.ends_with("_mint")),
        ),
        is_burn: u8::from(
            transaction
                .relationships
                .iter()
                .any(|relationship| relationship.transfer_type.ends_with("_burn")),
        ),
        is_liquidity_add,
        is_liquidity_remove,
        is_contract_call: u8::from(
            transaction.destination.is_some() && !transaction.input_selector.is_empty(),
        ),
        unique_assets: u16::try_from(assets.len()).unwrap_or(u16::MAX),
        participants: u16::try_from(participants.len()).unwrap_or(u16::MAX),
        classification_confidence: events
            .iter()
            .map(|event| event.confidence)
            .fold(0.0_f32, f32::max),
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        evidence_refs,
    }
}

fn swap_family(topic0: &B256) -> Option<&'static str> {
    if topic0 == &*SWAP_V2_EVENT {
        Some("uniswap_v2_compatible")
    } else if topic0 == &*SWAP_V3_EVENT {
        Some("uniswap_v3_compatible")
    } else if topic0 == &*CURVE_SWAP_EVENT {
        Some("curve_token_exchange")
    } else if topic0 == &*CURVE_UNDERLYING_SWAP_EVENT {
        Some("curve_underlying_exchange")
    } else if topic0 == &*BALANCER_SWAP_EVENT {
        Some("balancer_vault")
    } else {
        None
    }
}

fn decode_balancer_assets(network_id: &str, log: &Log) -> Option<(String, String, String, String)> {
    let topics = log.topics();
    let token_in = topic_address(*topics.get(2)?)?;
    let token_out = topic_address(*topics.get(3)?)?;
    let amount_in = U256::from_be_slice(word(log, 0)?);
    let amount_out = U256::from_be_slice(word(log, 1)?);
    Some((
        evm_asset(network_id, "erc20", token_in),
        amount_in.to_string(),
        evm_asset(network_id, "erc20", token_out),
        amount_out.to_string(),
    ))
}

fn infer_pool_flows(
    movements: &[ObservedMovement],
    pool: Address,
) -> Option<(String, String, String, String)> {
    let mut incoming = HashMap::<&str, U256>::new();
    let mut outgoing = HashMap::<&str, U256>::new();
    for movement in movements {
        if movement.to == pool && movement.from != pool {
            add_amount(&mut incoming, &movement.asset_id, movement.amount);
        }
        if movement.from == pool && movement.to != pool {
            add_amount(&mut outgoing, &movement.asset_id, movement.amount);
        }
    }
    if incoming.len() != 1 || outgoing.len() != 1 {
        return None;
    }
    let (asset_in, amount_in) = incoming.into_iter().next()?;
    let (asset_out, amount_out) = outgoing.into_iter().next()?;
    Some((
        asset_in.to_string(),
        amount_in.to_string(),
        asset_out.to_string(),
        amount_out.to_string(),
    ))
}

fn add_amount<'a>(amounts: &mut HashMap<&'a str, U256>, asset: &'a str, amount: U256) {
    let total = amounts.entry(asset).or_default();
    *total = total.checked_add(amount).unwrap_or(U256::MAX);
}

fn word(log: &Log, index: usize) -> Option<&[u8]> {
    let start = index.checked_mul(32)?;
    log.inner.data.data.get(start..start.checked_add(32)?)
}

fn word_address(word: &[u8]) -> Option<Address> {
    (word.len() == 32 && word[..12].iter().all(|byte| *byte == 0))
        .then(|| Address::from_slice(&word[12..]))
}

fn topic_address(topic: B256) -> Option<Address> {
    word_address(topic.as_slice())
}

fn signature(value: &str) -> B256 {
    keccak256(value.as_bytes())
}

fn is_any(topic: &B256, candidates: &[&LazyLock<B256>]) -> bool {
    candidates.iter().any(|candidate| topic == &***candidate)
}

fn evm_asset(network_id: &str, standard: &str, address: Address) -> String {
    format!("{network_id}/{standard}:{address:#x}")
}

fn semantic_event_id(
    network_id: &str,
    tx_hash: B256,
    log_index: u32,
    event_type: &str,
    emitter: Address,
) -> String {
    stable_hash(&format!(
        "{network_id}|{tx_hash:#x}|{log_index}|{event_type}|{emitter:#x}|{DETECTOR_VERSION}"
    ))
}

#[allow(clippy::too_many_arguments)]
fn bridge_correlation_key(
    protocol: &str,
    local_network: &str,
    remote_network: &str,
    local_asset: &str,
    remote_asset: &str,
    from: Address,
    to: Address,
    amount: U256,
) -> String {
    let mut endpoints = [
        format!("{local_network}|{local_asset}"),
        format!("{remote_network}|{remote_asset}"),
    ];
    endpoints.sort();
    stable_hash(&format!(
        "{protocol}|{}|{}|{from:#x}|{to:#x}|{amount}",
        endpoints[0], endpoints[1]
    ))
}

fn stable_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn sorted_join<'a>(values: HashSet<&'a str>) -> String {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    values.join(",")
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{B256, Bytes, Log as PrimitiveLog, U256, address};
    use alloy::rpc::types::Log;

    use super::{
        BALANCER_SWAP_EVENT, OP_ERC20_BRIDGE_INITIATED, ObservedMovement, SemanticDecoderRegistry,
        SemanticTransaction, decode_transaction, infer_pool_flows, swap_family,
    };
    use crate::storage::ProtocolContractRow;

    #[test]
    fn recognizes_supported_swap_signature() {
        assert_eq!(swap_family(&BALANCER_SWAP_EVENT), Some("balancer_vault"));
    }

    #[test]
    fn correlates_single_pool_inflow_and_outflow() {
        let pool = address!("0000000000000000000000000000000000000010");
        let wallet = address!("0000000000000000000000000000000000000020");
        let recipient = address!("0000000000000000000000000000000000000030");
        let token_in = address!("0000000000000000000000000000000000000040");
        let token_out = address!("0000000000000000000000000000000000000050");
        let movements = vec![
            ObservedMovement {
                log_index: 1,
                contract: token_in,
                from: wallet,
                to: pool,
                asset_id: format!("eip155:1/erc20:{token_in:#x}"),
                amount: U256::from(10_u64),
            },
            ObservedMovement {
                log_index: 2,
                contract: token_out,
                from: pool,
                to: recipient,
                asset_id: format!("eip155:1/erc20:{token_out:#x}"),
                amount: U256::from(9_u64),
            },
        ];

        let (_, amount_in, _, amount_out) = infer_pool_flows(&movements, pool).unwrap();
        assert_eq!(amount_in, "10");
        assert_eq!(amount_out, "9");
    }

    #[test]
    fn decodes_registered_op_erc20_bridge_event() {
        let bridge = address!("99c9fc46f92e8a1c0dec1b1747d010903e884be1");
        let local_token = address!("0000000000000000000000000000000000000040");
        let remote_token = address!("0000000000000000000000000000000000000050");
        let from = address!("0000000000000000000000000000000000000020");
        let to = address!("0000000000000000000000000000000000000030");
        let amount = U256::from(25_u64);
        let mut data = Vec::new();
        let mut to_word = [0_u8; 32];
        to_word[12..].copy_from_slice(to.as_slice());
        data.extend_from_slice(&to_word);
        data.extend_from_slice(&amount.to_be_bytes::<32>());
        data.extend_from_slice(&U256::from(96_u64).to_be_bytes::<32>());
        data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
        let log = Log {
            inner: PrimitiveLog::new_unchecked(
                bridge,
                vec![
                    *OP_ERC20_BRIDGE_INITIATED,
                    address_topic(local_token),
                    address_topic(remote_token),
                    address_topic(from),
                ],
                Bytes::from(data),
            ),
            log_index: Some(9),
            ..Default::default()
        };
        let registry = SemanticDecoderRegistry::from_contracts(vec![ProtocolContractRow {
            network_id: "eip155:1".to_string(),
            contract_address: format!("{bridge:#x}"),
            protocol: "optimism_standard_bridge".to_string(),
            protocol_type: "bridge".to_string(),
            contract_role: "l1_standard_bridge".to_string(),
            remote_network_id: "eip155:10".to_string(),
            remote_contract_address: "0x4200000000000000000000000000000000000010".to_string(),
            decoder: "op_standard_bridge_v1".to_string(),
            source: "official".to_string(),
            confidence: 1.0,
            enabled: 1,
            created_at_unix_ms: 1,
        }]);
        let logs = vec![log];
        let movements = Vec::new();
        let relationships = Vec::new();

        let decoded = decode_transaction(
            &registry,
            SemanticTransaction {
                network_id: "eip155:1",
                block_number: 100,
                block_timestamp_unix_ms: 1_700_000_000_000,
                tx_hash: B256::repeat_byte(0xaa),
                sender: from,
                destination: Some(bridge),
                input_selector: "0x12345678",
                logs: &logs,
                movements: &movements,
                relationships: &relationships,
            },
        );

        assert_eq!(decoded.events.len(), 1);
        assert_eq!(decoded.events[0].event_type, "bridge_transfer");
        assert_eq!(decoded.events[0].bridge_direction, "outbound");
        assert_eq!(decoded.events[0].remote_network_id, "eip155:10");
        assert_eq!(decoded.events[0].amount_in, "25");
        assert!(!decoded.events[0].correlation_key.is_empty());
        assert_eq!(decoded.transaction_features.len(), 1);
        assert_eq!(decoded.transaction_features[0].is_bridge, 1);
    }

    fn address_topic(address: alloy::primitives::Address) -> B256 {
        let mut bytes = [0_u8; 32];
        bytes[12..].copy_from_slice(address.as_slice());
        bytes.into()
    }
}
