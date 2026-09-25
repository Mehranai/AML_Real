use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    str::FromStr,
};

use clickhouse::Row;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    BSC_NETWORK_ID,
    domain::NetworkId,
    ingestion::{
        CanonicalBlock, CanonicalRelationship, CanonicalTransaction, ExtractedBlockEvidence,
        normalize_address,
    },
};

const DETECTOR: &str = "bsc_semantic_classifier";
const DETECTOR_VERSION: &str = "bsc_semantic_v2_exact_flows";
mod flow_amount {
    // At most 256 UInt256 legs are summed per bounded observation.
    uint::construct_uint! { pub struct U512(8); }
}
use flow_amount::U512;
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
const SWAP_V2_TOPIC: &str = "0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822";
const SWAP_V3_TOPIC: &str = "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";
const CURVE_SWAP_TOPIC: &str = "0x8b3e96f2b889fa771c53c981b40daf005f63f637f1869f707052d15a3dd97140";
const CURVE_UNDERLYING_SWAP_TOPIC: &str =
    "0xd013ca23e77a65003c2c659c5442c00c805371b7fc1ebd4c206c41d1536bd90b";
const BALANCER_SWAP_TOPIC: &str =
    "0x2170c741c41531aec20e7c107c24eecfdd15e69c9bb0a8dd37b1840b9e0b207b";
const V2_MINT_TOPIC: &str = "0x4c209b5fc8ad50758f13e2e1088ba56a560dff690a1c6fef26394f4c03821c4f";
const V2_BURN_TOPIC: &str = "0xdccd412f0b1252819cb1fd330b93224ca42612892bb3f4f789976e6d81936496";
const V3_MINT_TOPIC: &str = "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde";
const V3_BURN_TOPIC: &str = "0x0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c";
const MIXER_DEPOSIT_TOPIC: &str =
    "0xa945e51eec50ab98c161376f0db4cf2aeba3ec92755fe2fcd388bdbbb80ff196";
const MIXER_WITHDRAWAL_TOPIC: &str =
    "0xe9e508bad6d4c3227e881ca19068f099da81b5164dd6d62b2eaf1e8bc6c34931";

const PROTOCOL_TYPES: &[&str] = &[
    "dex", "bridge", "lending", "staking", "mixer", "scam", "system",
];
const DECODERS: &[&str] = &[
    "amm_v2",
    "amm_v3",
    "aggregator",
    "bridge_generic",
    "lending_generic",
    "staking_generic",
    "tornado_cash_v1",
    "mixer_generic",
    "reviewed_interaction",
    "bsc_system",
];
const REVIEW_STATUSES: &[&str] = &["pending", "approved", "rejected"];
const EVENT_TYPES: &[&str] = &[
    "swap",
    "bridge_deposit",
    "bridge_withdrawal",
    "liquidity_add",
    "liquidity_remove",
    "lending_supply",
    "lending_withdrawal",
    "lending_borrow",
    "lending_repay",
    "lending_liquidation",
    "staking_deposit",
    "staking_withdrawal",
    "staking_claim",
    "mixer_deposit",
    "mixer_withdrawal",
    "scam_interaction",
    "bsc_system_transaction",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolRegistryInput {
    pub contract_address: String,
    pub protocol: String,
    pub protocol_type: String,
    pub contract_role: String,
    pub decoder: String,
    #[serde(default)]
    pub remote_network_id: String,
    #[serde(default)]
    pub remote_contract_address: String,
    #[serde(default)]
    pub method_ids: Vec<String>,
    #[serde(default)]
    pub method_event_types: Vec<String>,
    #[serde(default)]
    pub event_topics: Vec<String>,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default = "unknown_topic_index")]
    pub remote_receiver_topic_index: i8,
    #[serde(default = "unknown_topic_index")]
    pub message_topic_index: i8,
    pub source_id: String,
    #[serde(default)]
    pub source_reference: String,
    pub review_status: String,
    #[serde(default = "default_confidence")]
    pub evidence_confidence: f32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub reviewed_by: String,
    #[serde(default)]
    pub review_note: String,
}

fn unknown_topic_index() -> i8 {
    -1
}

fn default_confidence() -> f32 {
    1.0
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq)]
pub struct ProtocolContract {
    pub network_id: String,
    pub contract_address: String,
    pub protocol: String,
    pub protocol_type: String,
    pub contract_role: String,
    pub decoder: String,
    pub remote_network_id: String,
    pub remote_contract_address: String,
    pub method_ids: Vec<String>,
    pub method_event_types: Vec<String>,
    pub event_topics: Vec<String>,
    pub event_types: Vec<String>,
    pub remote_receiver_topic_index: i8,
    pub message_topic_index: i8,
    pub source_id: String,
    pub source_reference: String,
    pub review_status: String,
    pub evidence_confidence: f32,
    pub enabled: u8,
    pub registry_revision: u64,
    pub reviewed_by: String,
    pub review_note: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolImportResult {
    pub contract_address: String,
    pub registry_revision: u64,
    pub review_status: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SemanticRegistry {
    contracts: HashMap<String, ProtocolContract>,
}

impl SemanticRegistry {
    pub(crate) fn from_contracts(contracts: Vec<ProtocolContract>) -> Self {
        Self {
            contracts: contracts
                .into_iter()
                .filter(|record| record.network_id == BSC_NETWORK_ID)
                .filter(|record| record.review_status == "approved" && record.enabled == 1)
                .map(|record| (record.contract_address.clone(), record))
                .collect(),
        }
    }

    fn matching<'a>(
        &'a self,
        transaction: &CanonicalTransaction,
        log_addresses: impl Iterator<Item = &'a str>,
    ) -> Vec<&'a ProtocolContract> {
        let mut addresses = BTreeSet::new();
        if !transaction.to_address.is_empty() {
            addresses.insert(transaction.to_address.clone());
        }
        addresses.extend(log_addresses.map(str::to_string));
        if self
            .contracts
            .get(&transaction.from_address)
            .is_some_and(|record| record.protocol_type == "system")
        {
            addresses.insert(transaction.from_address.clone());
        }
        addresses
            .into_iter()
            .filter_map(|address| self.contracts.get(&address))
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SemanticBlockEvidence {
    pub transaction_features: Vec<TransactionFeature>,
    pub events: Vec<SemanticEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TransactionFeature {
    pub feature_id: String,
    pub tx_hash: String,
    pub transaction_type: String,
    pub transaction_subtype: String,
    pub protocol: String,
    pub method_id: String,
    pub is_swap: u8,
    pub is_bridge: u8,
    pub is_mint: u8,
    pub is_burn: u8,
    pub is_liquidity_add: u8,
    pub is_liquidity_remove: u8,
    pub is_contract_call: u8,
    pub unique_assets: u16,
    pub participants: u16,
    pub classification_confidence: f32,
    pub classification_source: String,
    pub detector: String,
    pub detector_version: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SemanticEvent {
    pub event_id: String,
    pub tx_hash: String,
    pub event_index: u32,
    pub event_type: String,
    pub subject_address: String,
    pub protocol: String,
    pub protocol_contract: String,
    pub counterparty_address: String,
    pub correlation_key: String,
    pub asset_in: String,
    pub asset_out: String,
    pub remote_network_id: String,
    pub remote_asset: String,
    pub bridge_direction: String,
    pub remote_receiver: String,
    pub bridge_message_id: String,
    pub amount_in: String,
    pub amount_out: String,
    pub detector: String,
    pub detector_version: String,
    pub confidence: f32,
    pub evidence_refs: Vec<String>,
    pub evidence_json: String,
    classification_source: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedProtocolContract {
    pub contract_address: String,
    pub protocol: String,
    pub protocol_type: String,
    pub contract_role: String,
    pub decoder: String,
    pub remote_network_id: String,
    pub remote_contract_address: String,
    pub method_ids: Vec<String>,
    pub method_event_types: Vec<String>,
    pub event_topics: Vec<String>,
    pub event_types: Vec<String>,
    pub remote_receiver_topic_index: i8,
    pub message_topic_index: i8,
    pub source_id: String,
    pub source_reference: String,
    pub review_status: String,
    pub evidence_confidence: f32,
    pub enabled: u8,
    pub reviewed_by: String,
    pub review_note: String,
}

impl ProtocolRegistryInput {
    pub fn validate_only(&self) -> Result<String, RegistryValidationError> {
        self.clone()
            .validate()
            .map(|record| record.contract_address)
    }

    pub(crate) fn validate(self) -> Result<ValidatedProtocolContract, RegistryValidationError> {
        let contract_address =
            normalize_address("registry.contract_address", &self.contract_address)
                .map_err(|_| RegistryValidationError::InvalidAddress("contract_address"))?;
        validate_text("protocol", &self.protocol, 128, true)?;
        validate_choice("protocol_type", &self.protocol_type, PROTOCOL_TYPES)?;
        validate_text("contract_role", &self.contract_role, 64, true)?;
        validate_choice("decoder", &self.decoder, DECODERS)?;
        validate_decoder_compatibility(&self.protocol_type, &self.decoder)?;
        validate_text("source_id", &self.source_id, 128, true)?;
        validate_text("source_reference", &self.source_reference, 1_024, false)?;
        validate_choice("review_status", &self.review_status, REVIEW_STATUSES)?;
        validate_text("reviewed_by", &self.reviewed_by, 128, false)?;
        validate_text("review_note", &self.review_note, 1_024, false)?;
        if self.review_status == "approved" && self.reviewed_by.is_empty() {
            return Err(RegistryValidationError::MissingReviewer);
        }
        if !self.evidence_confidence.is_finite() || !(0.0..=1.0).contains(&self.evidence_confidence)
        {
            return Err(RegistryValidationError::InvalidConfidence);
        }
        if self.method_ids.len() != self.method_event_types.len() {
            return Err(RegistryValidationError::MismatchedMappings("method"));
        }
        if self.event_topics.len() != self.event_types.len() {
            return Err(RegistryValidationError::MismatchedMappings("event"));
        }
        if self.method_ids.len() > 128 || self.event_topics.len() > 128 {
            return Err(RegistryValidationError::TooManyMappings);
        }
        validate_unique_mapping(&self.method_ids, &self.method_event_types, 4, "method")?;
        validate_unique_mapping(&self.event_topics, &self.event_types, 32, "event")?;
        for event_type in self
            .method_event_types
            .iter()
            .chain(self.event_types.iter())
        {
            validate_choice("event_type", event_type, EVENT_TYPES)?;
            validate_event_compatibility(&self.protocol_type, event_type)?;
        }
        for index in [self.remote_receiver_topic_index, self.message_topic_index] {
            if !(-1..=15).contains(&index) {
                return Err(RegistryValidationError::InvalidTopicIndex);
            }
        }
        let remote_network_id = if self.remote_network_id.is_empty() {
            String::new()
        } else {
            NetworkId::from_str(&self.remote_network_id)
                .map_err(|_| RegistryValidationError::InvalidNetwork)?
                .to_string()
        };
        let remote_contract_address = if self.remote_contract_address.is_empty() {
            String::new()
        } else {
            normalize_address(
                "registry.remote_contract_address",
                &self.remote_contract_address,
            )
            .map_err(|_| RegistryValidationError::InvalidAddress("remote_contract_address"))?
        };

        Ok(ValidatedProtocolContract {
            contract_address,
            protocol: self.protocol,
            protocol_type: self.protocol_type,
            contract_role: self.contract_role,
            decoder: self.decoder,
            remote_network_id,
            remote_contract_address,
            method_ids: normalize_hex_mappings(self.method_ids),
            method_event_types: self.method_event_types,
            event_topics: normalize_hex_mappings(self.event_topics),
            event_types: self.event_types,
            remote_receiver_topic_index: self.remote_receiver_topic_index,
            message_topic_index: self.message_topic_index,
            source_id: self.source_id,
            source_reference: self.source_reference,
            review_status: self.review_status,
            evidence_confidence: self.evidence_confidence,
            enabled: u8::from(self.enabled),
            reviewed_by: self.reviewed_by,
            review_note: self.review_note,
        })
    }
}

pub(crate) fn classify_block(
    registry: &SemanticRegistry,
    block: &CanonicalBlock,
    extracted: &ExtractedBlockEvidence,
) -> SemanticBlockEvidence {
    let mut output = SemanticBlockEvidence::default();
    for transaction in block
        .transactions
        .iter()
        .filter(|transaction| transaction.status == 1)
    {
        let logs = block
            .logs
            .iter()
            .filter(|log| log.tx_hash == transaction.hash)
            .collect::<Vec<_>>();
        let relationships = extracted
            .relationships
            .iter()
            .filter(|relationship| relationship.tx_hash == transaction.hash)
            .collect::<Vec<_>>();
        let mut events = token_lifecycle_events(transaction, &relationships);
        let rules = registry.matching(
            transaction,
            logs.iter().map(|log| log.contract_address.as_str()),
        );
        for rule in rules {
            classify_protocol(transaction, &logs, &relationships, rule, &mut events);
        }
        events.sort_by(|left, right| {
            (&left.event_type, left.event_index, &left.event_id).cmp(&(
                &right.event_type,
                right.event_index,
                &right.event_id,
            ))
        });
        events.dedup_by(|left, right| left.event_id == right.event_id);
        if !events.is_empty() {
            output.transaction_features.push(transaction_feature(
                transaction,
                &relationships,
                &events,
            ));
            output.events.extend(events);
        }
    }
    output
}

fn classify_protocol(
    transaction: &CanonicalTransaction,
    logs: &[&crate::ingestion::CanonicalLog],
    relationships: &[&CanonicalRelationship],
    rule: &ProtocolContract,
    events: &mut Vec<SemanticEvent>,
) {
    let flow = subject_flow(
        &transaction.from_address,
        &rule.contract_address,
        relationships,
    );
    let mut emitted = HashSet::new();

    if let Some(position) = rule
        .method_ids
        .iter()
        .position(|method| method == &transaction.input_selector)
    {
        let event_type = &rule.method_event_types[position];
        push_registry_event(
            events,
            &mut emitted,
            transaction,
            rule,
            event_type,
            transaction.transaction_index,
            None,
            &flow,
            "reviewed_registry:method",
            0.88,
        );
    }

    for log in logs
        .iter()
        .filter(|log| log.contract_address == rule.contract_address)
    {
        if let Some(position) = rule
            .event_topics
            .iter()
            .position(|topic| topic == &log.topic0)
        {
            let event_type = &rule.event_types[position];
            push_registry_event(
                events,
                &mut emitted,
                transaction,
                rule,
                event_type,
                log.log_index,
                Some(log),
                &flow,
                "reviewed_registry:event_topic",
                0.98,
            );
        }
    }

    match rule.decoder.as_str() {
        "amm_v2" | "amm_v3" | "aggregator" => {
            for log in logs {
                let standard_event = match log.topic0.as_str() {
                    SWAP_V2_TOPIC if rule.decoder != "amm_v3" => Some("swap"),
                    SWAP_V3_TOPIC if rule.decoder != "amm_v2" => Some("swap"),
                    CURVE_SWAP_TOPIC | CURVE_UNDERLYING_SWAP_TOPIC | BALANCER_SWAP_TOPIC
                        if rule.decoder == "aggregator" =>
                    {
                        Some("swap")
                    }
                    V2_MINT_TOPIC if rule.decoder != "amm_v3" => Some("liquidity_add"),
                    V2_BURN_TOPIC if rule.decoder != "amm_v3" => Some("liquidity_remove"),
                    V3_MINT_TOPIC if rule.decoder != "amm_v2" => Some("liquidity_add"),
                    V3_BURN_TOPIC if rule.decoder != "amm_v2" => Some("liquidity_remove"),
                    _ => None,
                };
                if let Some(event_type) = standard_event {
                    push_registry_event(
                        events,
                        &mut emitted,
                        transaction,
                        rule,
                        event_type,
                        log.log_index,
                        Some(log),
                        &flow,
                        "reviewed_registry:standard_topic",
                        if flow.both_directions() { 0.98 } else { 0.90 },
                    );
                }
            }
            if rule.decoder == "aggregator"
                && emitted.is_empty()
                && flow.both_directions()
                && flow.unique_assets() >= 2
            {
                push_registry_event(
                    events,
                    &mut emitted,
                    transaction,
                    rule,
                    "swap",
                    transaction.transaction_index,
                    None,
                    &flow,
                    "reviewed_registry:fund_flow",
                    0.76,
                );
            }
        }
        "bridge_generic" => {
            if emitted.is_empty() {
                let event_type = if flow.has_incoming_to_protocol() {
                    Some("bridge_deposit")
                } else if flow.has_outgoing_from_protocol() {
                    Some("bridge_withdrawal")
                } else {
                    None
                };
                if let Some(event_type) = event_type {
                    push_registry_event(
                        events,
                        &mut emitted,
                        transaction,
                        rule,
                        event_type,
                        transaction.transaction_index,
                        None,
                        &flow,
                        "reviewed_registry:fund_flow",
                        0.74,
                    );
                }
            }
        }
        "lending_generic" => push_directional_fallback(
            events,
            &mut emitted,
            transaction,
            rule,
            &flow,
            "lending_supply",
            "lending_withdrawal",
        ),
        "staking_generic" => push_directional_fallback(
            events,
            &mut emitted,
            transaction,
            rule,
            &flow,
            "staking_deposit",
            "staking_withdrawal",
        ),
        "tornado_cash_v1" => {
            for log in logs
                .iter()
                .filter(|log| log.contract_address == rule.contract_address)
            {
                let event_type = match log.topic0.as_str() {
                    MIXER_DEPOSIT_TOPIC => Some("mixer_deposit"),
                    MIXER_WITHDRAWAL_TOPIC => Some("mixer_withdrawal"),
                    _ => None,
                };
                if let Some(event_type) = event_type {
                    push_registry_event(
                        events,
                        &mut emitted,
                        transaction,
                        rule,
                        event_type,
                        log.log_index,
                        Some(log),
                        &flow,
                        "reviewed_registry:standard_topic",
                        0.99,
                    );
                }
            }
        }
        "mixer_generic" => push_directional_fallback(
            events,
            &mut emitted,
            transaction,
            rule,
            &flow,
            "mixer_deposit",
            "mixer_withdrawal",
        ),
        "reviewed_interaction" => push_registry_event(
            events,
            &mut emitted,
            transaction,
            rule,
            "scam_interaction",
            transaction.transaction_index,
            None,
            &flow,
            "reviewed_registry:interaction",
            1.0,
        ),
        "bsc_system" => push_registry_event(
            events,
            &mut emitted,
            transaction,
            rule,
            "bsc_system_transaction",
            transaction.transaction_index,
            None,
            &flow,
            "reviewed_registry:system_actor",
            1.0,
        ),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn push_registry_event(
    events: &mut Vec<SemanticEvent>,
    emitted: &mut HashSet<String>,
    transaction: &CanonicalTransaction,
    rule: &ProtocolContract,
    event_type: &str,
    event_index: u32,
    log: Option<&crate::ingestion::CanonicalLog>,
    flow: &FlowSummary,
    classification_source: &str,
    evidence_quality: f32,
) {
    let protocol_contract = log
        .map(|item| item.contract_address.as_str())
        .unwrap_or(&rule.contract_address);
    let event_id = stable_id(&format!(
        "{BSC_NETWORK_ID}|{DETECTOR_VERSION}|{}|{event_type}|{protocol_contract}|{event_index}",
        transaction.hash
    ));
    if !emitted.insert(event_id.clone()) {
        return;
    }
    let inbound = is_protocol_inbound(event_type);
    let outbound = is_protocol_outbound(event_type);
    let remote_receiver = log
        .and_then(|item| topic_at(item, rule.remote_receiver_topic_index))
        .and_then(topic_address)
        .unwrap_or_default();
    let bridge_message_id = log
        .and_then(|item| topic_at(item, rule.message_topic_index))
        .unwrap_or_default()
        .to_string();
    let bridge_direction = match event_type {
        "bridge_deposit" => "outbound",
        "bridge_withdrawal" => "inbound",
        _ => "",
    };
    let subject_address = if event_type == "mixer_withdrawal" {
        log.and_then(|item| topic_at(item, 1))
            .and_then(topic_address)
            .or_else(|| flow.recipient.clone())
            .unwrap_or_else(|| transaction.from_address.clone())
    } else if event_type == "bridge_withdrawal" {
        (!remote_receiver.is_empty())
            .then(|| remote_receiver.clone())
            .or_else(|| flow.recipient.clone())
            .unwrap_or_else(|| transaction.from_address.clone())
    } else {
        transaction.from_address.clone()
    };
    let mut evidence_refs = vec![format!("tx:{}", transaction.hash)];
    if let Some(log) = log {
        evidence_refs.push(log.event_id.clone());
    }
    evidence_refs.extend(flow.evidence_refs.iter().take(64).cloned());
    evidence_refs.sort();
    evidence_refs.dedup();
    let confidence = rule.evidence_confidence.min(evidence_quality);
    let correlation_key = if !bridge_message_id.is_empty() {
        bridge_message_id.clone()
    } else if event_type.starts_with("bridge_") {
        stable_id(&format!(
            "{}|{}|{}|{}|{}",
            rule.protocol,
            transaction.hash,
            bridge_direction,
            rule.remote_network_id,
            remote_receiver
        ))
    } else {
        String::new()
    };
    let evidence_json = json!({
        "confidence_semantics": "evidence_quality_not_illicit_probability",
        "classification_source": classification_source,
        "registry": {
            "revision": rule.registry_revision,
            "source_id": rule.source_id,
            "source_reference": rule.source_reference,
            "review_status": rule.review_status,
            "reviewed_by": rule.reviewed_by,
            "contract_role": rule.contract_role,
            "decoder": rule.decoder,
        },
        "signal": {
            "method_id": transaction.input_selector,
            "topic0": log.map(|item| item.topic0.as_str()).unwrap_or(""),
            "log_index": log.map(|item| item.log_index),
            "flow_edges": flow.evidence_refs.len(),
            "flow_boundary": flow.boundary,
            "flow_truncated": flow.truncated,
            "flow_evidence_refs_truncated": flow.evidence_refs.len() > 64,
            "nft_legs_excluded": flow.nft_legs_excluded,
            "remote_receiver": remote_receiver,
            "bridge_message_id": bridge_message_id,
        }
    })
    .to_string();

    events.push(SemanticEvent {
        event_id,
        tx_hash: transaction.hash.clone(),
        event_index,
        event_type: event_type.to_string(),
        subject_address,
        protocol: rule.protocol.clone(),
        protocol_contract: protocol_contract.to_string(),
        counterparty_address: protocol_contract.to_string(),
        correlation_key,
        asset_in: if inbound {
            flow.assets_in.clone()
        } else {
            String::new()
        },
        asset_out: if outbound {
            flow.assets_out.clone()
        } else {
            String::new()
        },
        remote_network_id: if event_type.starts_with("bridge_") {
            rule.remote_network_id.clone()
        } else {
            String::new()
        },
        remote_asset: String::new(),
        bridge_direction: bridge_direction.to_string(),
        remote_receiver,
        bridge_message_id,
        amount_in: if inbound {
            flow.amounts_in.clone()
        } else {
            String::new()
        },
        amount_out: if outbound {
            flow.amounts_out.clone()
        } else {
            String::new()
        },
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        confidence,
        evidence_refs,
        evidence_json,
        classification_source: classification_source.to_string(),
    });
}

fn push_directional_fallback(
    events: &mut Vec<SemanticEvent>,
    emitted: &mut HashSet<String>,
    transaction: &CanonicalTransaction,
    rule: &ProtocolContract,
    flow: &FlowSummary,
    inbound_type: &str,
    outbound_type: &str,
) {
    if !emitted.is_empty() {
        return;
    }
    let event_type = if flow.has_incoming_to_protocol() {
        Some(inbound_type)
    } else if flow.has_outgoing_from_protocol() {
        Some(outbound_type)
    } else {
        None
    };
    if let Some(event_type) = event_type {
        push_registry_event(
            events,
            emitted,
            transaction,
            rule,
            event_type,
            transaction.transaction_index,
            None,
            flow,
            "reviewed_registry:fund_flow",
            0.72,
        );
    }
}

fn token_lifecycle_events(
    transaction: &CanonicalTransaction,
    relationships: &[&CanonicalRelationship],
) -> Vec<SemanticEvent> {
    relationships
        .iter()
        .filter_map(|relationship| {
            let event_type = if relationship.transfer_type.starts_with("erc")
                && relationship.from_address == ZERO_ADDRESS
            {
                "token_mint"
            } else if relationship.transfer_type.starts_with("erc")
                && relationship.to_address == ZERO_ADDRESS
            {
                "token_burn"
            } else {
                return None;
            };
            let subject = if event_type == "token_mint" {
                &relationship.to_address
            } else {
                &relationship.from_address
            };
            let contract = token_contract_from_asset(&relationship.asset_id).unwrap_or_default();
            let event_id = stable_id(&format!(
                "{BSC_NETWORK_ID}|{DETECTOR_VERSION}|{}|{event_type}|{}",
                transaction.hash, relationship.relationship_id
            ));
            Some(SemanticEvent {
                event_id,
                tx_hash: transaction.hash.clone(),
                event_index: relationship.event_index,
                event_type: event_type.to_string(),
                subject_address: subject.clone(),
                protocol: "token_contract".to_string(),
                protocol_contract: contract,
                counterparty_address: ZERO_ADDRESS.to_string(),
                correlation_key: String::new(),
                asset_in: if event_type == "token_mint" {
                    relationship.asset_id.clone()
                } else {
                    String::new()
                },
                asset_out: if event_type == "token_burn" {
                    relationship.asset_id.clone()
                } else {
                    String::new()
                },
                remote_network_id: String::new(),
                remote_asset: String::new(),
                bridge_direction: String::new(),
                remote_receiver: String::new(),
                bridge_message_id: String::new(),
                amount_in: if event_type == "token_mint" {
                    relationship.amount.decimal_string()
                } else {
                    String::new()
                },
                amount_out: if event_type == "token_burn" {
                    relationship.amount.decimal_string()
                } else {
                    String::new()
                },
                detector: DETECTOR.to_string(),
                detector_version: DETECTOR_VERSION.to_string(),
                confidence: 1.0,
                evidence_refs: vec![relationship.relationship_id.clone()],
                evidence_json: json!({
                    "confidence_semantics": "evidence_quality_not_illicit_probability",
                    "classification_source": "canonical_transfer:zero_address",
                    "transfer_type": relationship.transfer_type,
                    "token_id": relationship.token_id,
                })
                .to_string(),
                classification_source: "canonical_transfer:zero_address".to_string(),
            })
        })
        .collect()
}

fn transaction_feature(
    transaction: &CanonicalTransaction,
    relationships: &[&CanonicalRelationship],
    events: &[SemanticEvent],
) -> TransactionFeature {
    let event_types = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<BTreeSet<_>>();
    let protocols = events
        .iter()
        .map(|event| event.protocol.as_str())
        .collect::<BTreeSet<_>>();
    let assets = relationships
        .iter()
        .map(|relationship| relationship.asset_id.as_str())
        .collect::<HashSet<_>>();
    let participants = relationships
        .iter()
        .flat_map(|relationship| {
            [
                relationship.from_address.as_str(),
                relationship.to_address.as_str(),
            ]
        })
        .filter(|address| *address != ZERO_ADDRESS)
        .collect::<HashSet<_>>();
    let strongest = events
        .iter()
        .max_by(|left, right| left.confidence.total_cmp(&right.confidence))
        .expect("feature is only built for non-empty events");
    let transaction_type = if event_types.contains("bsc_system_transaction") {
        "system"
    } else if event_types.contains("scam_interaction") {
        "intelligence"
    } else if event_types.iter().any(|event| event.starts_with("mixer_")) {
        "privacy"
    } else if event_types.iter().any(|event| event.starts_with("bridge_")) {
        "bridge"
    } else if event_types.iter().any(|event| {
        event.starts_with("lending_")
            || event.starts_with("staking_")
            || *event == "swap"
            || event.starts_with("liquidity_")
    }) {
        "defi"
    } else {
        "token"
    };
    let mut evidence_refs = events
        .iter()
        .flat_map(|event| event.evidence_refs.iter().cloned())
        .collect::<Vec<_>>();
    evidence_refs.sort();
    evidence_refs.dedup();

    TransactionFeature {
        feature_id: stable_id(&format!(
            "{BSC_NETWORK_ID}|{DETECTOR_VERSION}|feature|{}",
            transaction.hash
        )),
        tx_hash: transaction.hash.clone(),
        transaction_type: transaction_type.to_string(),
        transaction_subtype: event_types.iter().next().unwrap_or(&"").to_string(),
        protocol: protocols.into_iter().collect::<Vec<_>>().join(","),
        method_id: transaction.input_selector.clone(),
        is_swap: u8::from(event_types.contains("swap")),
        is_bridge: u8::from(event_types.iter().any(|event| event.starts_with("bridge_"))),
        is_mint: u8::from(event_types.contains("token_mint")),
        is_burn: u8::from(event_types.contains("token_burn")),
        is_liquidity_add: u8::from(event_types.contains("liquidity_add")),
        is_liquidity_remove: u8::from(event_types.contains("liquidity_remove")),
        is_contract_call: u8::from(
            !transaction.to_address.is_empty() && transaction.input_data != "0x",
        ),
        unique_assets: u16::try_from(assets.len()).unwrap_or(u16::MAX),
        participants: u16::try_from(participants.len()).unwrap_or(u16::MAX),
        classification_confidence: strongest.confidence,
        classification_source: strongest.classification_source.clone(),
        detector: DETECTOR.to_string(),
        detector_version: DETECTOR_VERSION.to_string(),
        evidence_refs,
    }
}

#[derive(Debug, Default)]
struct FlowSummary {
    assets_in: String,
    amounts_in: String,
    assets_out: String,
    amounts_out: String,
    evidence_refs: Vec<String>,
    recipient: Option<String>,
    boundary: &'static str,
    truncated: bool,
    nft_legs_excluded: usize,
}

impl FlowSummary {
    fn has_incoming_to_protocol(&self) -> bool {
        !self.assets_in.is_empty()
    }

    fn has_outgoing_from_protocol(&self) -> bool {
        !self.assets_out.is_empty()
    }

    fn both_directions(&self) -> bool {
        self.has_incoming_to_protocol() && self.has_outgoing_from_protocol()
    }

    fn unique_assets(&self) -> usize {
        self.assets_in
            .split(',')
            .chain(self.assets_out.split(','))
            .filter(|asset| !asset.is_empty())
            .collect::<HashSet<_>>()
            .len()
    }
}

fn subject_flow(
    subject: &str,
    protocol_contract: &str,
    relationships: &[&CanonicalRelationship],
) -> FlowSummary {
    let mut incoming = BTreeMap::new();
    let mut outgoing = BTreeMap::new();
    let mut refs = Vec::new();
    let mut recipient = None;
    // Prefer the protocol boundary; combining user AND pool boundaries double-counts routed legs.
    let protocol_boundary = relationships
        .iter()
        .any(|r| r.from_address == protocol_contract || r.to_address == protocol_contract);
    let mut nft_legs_excluded = 0;
    for relationship in relationships.iter().take(256) {
        if !relationship.token_id.is_empty() {
            nft_legs_excluded += 1;
            continue;
        }
        if relationship.from_address == relationship.to_address || relationship.amount.is_zero() {
            continue;
        }
        let amount = U512::from_little_endian(&relationship.amount.as_clickhouse().to_le_bytes());
        let inbound = if protocol_boundary {
            relationship.to_address == protocol_contract
        } else {
            relationship.from_address == subject
        };
        let outbound = if protocol_boundary {
            relationship.from_address == protocol_contract
        } else {
            relationship.to_address == subject
        };
        if inbound && relationship.to_address != ZERO_ADDRESS {
            *incoming
                .entry(relationship.asset_id.clone())
                .or_insert(U512::zero()) += amount;
            refs.push(relationship.relationship_id.clone());
        }
        if outbound && relationship.from_address != ZERO_ADDRESS {
            *outgoing
                .entry(relationship.asset_id.clone())
                .or_insert(U512::zero()) += amount;
            refs.push(relationship.relationship_id.clone());
            if relationship.from_address == protocol_contract
                && relationship.to_address != protocol_contract
            {
                recipient.get_or_insert_with(|| relationship.to_address.clone());
            }
        }
    }
    let truncated = relationships.len() > 256 || incoming.len() > 32 || outgoing.len() > 32;
    refs.sort();
    refs.dedup();
    let (assets_in, amounts_in) = join_flows(incoming);
    let (assets_out, amounts_out) = join_flows(outgoing);
    FlowSummary {
        assets_in,
        amounts_in,
        assets_out,
        amounts_out,
        evidence_refs: refs,
        recipient,
        boundary: if protocol_boundary {
            "protocol"
        } else {
            "subject"
        },
        truncated,
        nft_legs_excluded,
    }
}

fn join_flows(flows: BTreeMap<String, U512>) -> (String, String) {
    let (assets, amounts): (Vec<_>, Vec<_>) = flows
        .into_iter()
        .take(32)
        .map(|(a, v)| (a, v.to_string()))
        .unzip();
    (assets.join(","), amounts.join(","))
}

fn is_protocol_inbound(event_type: &str) -> bool {
    matches!(
        event_type,
        "swap"
            | "bridge_deposit"
            | "liquidity_add"
            | "lending_supply"
            | "lending_repay"
            | "staking_deposit"
            | "mixer_deposit"
            | "token_mint"
    )
}

fn is_protocol_outbound(event_type: &str) -> bool {
    matches!(
        event_type,
        "swap"
            | "bridge_withdrawal"
            | "liquidity_remove"
            | "lending_withdrawal"
            | "lending_borrow"
            | "lending_liquidation"
            | "staking_withdrawal"
            | "staking_claim"
            | "mixer_withdrawal"
            | "token_burn"
    )
}

fn topic_at(log: &crate::ingestion::CanonicalLog, index: i8) -> Option<&str> {
    usize::try_from(index)
        .ok()
        .and_then(|index| log.topics.get(index))
        .map(String::as_str)
}

fn topic_address(topic: &str) -> Option<String> {
    (topic.len() == 66
        && topic.starts_with("0x")
        && topic[2..]
            .chars()
            .all(|character| character.is_ascii_hexdigit()))
    .then(|| format!("0x{}", &topic[26..].to_ascii_lowercase()))
}

fn token_contract_from_asset(asset: &str) -> Option<String> {
    let (_, contract) = asset.split_once("/erc")?;
    let (_, contract) = contract.split_once(':')?;
    Some(contract.split('/').next()?.to_string())
}

fn normalize_hex_mappings(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn validate_unique_mapping(
    keys: &[String],
    values: &[String],
    hex_bytes: usize,
    kind: &'static str,
) -> Result<(), RegistryValidationError> {
    let mut unique = HashSet::new();
    for (key, value) in keys.iter().zip(values) {
        if key.len() != 2 + hex_bytes * 2
            || !key.starts_with("0x")
            || !key[2..]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(RegistryValidationError::InvalidMappingHex(kind));
        }
        if !unique.insert(key.to_ascii_lowercase()) {
            return Err(RegistryValidationError::DuplicateMapping(kind));
        }
        validate_text("event_type", value, 64, true)?;
    }
    Ok(())
}

fn validate_text(
    field: &'static str,
    value: &str,
    max_length: usize,
    required: bool,
) -> Result<(), RegistryValidationError> {
    if (required && value.is_empty())
        || value.len() > max_length
        || value.chars().any(char::is_control)
    {
        return Err(RegistryValidationError::InvalidText(field));
    }
    Ok(())
}

fn validate_choice(
    field: &'static str,
    value: &str,
    choices: &'static [&'static str],
) -> Result<(), RegistryValidationError> {
    choices
        .contains(&value)
        .then_some(())
        .ok_or(RegistryValidationError::UnsupportedChoice { field })
}

fn validate_decoder_compatibility(
    protocol_type: &str,
    decoder: &str,
) -> Result<(), RegistryValidationError> {
    let valid = match protocol_type {
        "dex" => matches!(decoder, "amm_v2" | "amm_v3" | "aggregator"),
        "bridge" => decoder == "bridge_generic",
        "lending" => decoder == "lending_generic",
        "staking" => decoder == "staking_generic",
        "mixer" => matches!(decoder, "tornado_cash_v1" | "mixer_generic"),
        "scam" => decoder == "reviewed_interaction",
        "system" => decoder == "bsc_system",
        _ => false,
    };
    valid
        .then_some(())
        .ok_or(RegistryValidationError::IncompatibleDecoder)
}

fn validate_event_compatibility(
    protocol_type: &str,
    event_type: &str,
) -> Result<(), RegistryValidationError> {
    let valid = match protocol_type {
        "dex" => event_type == "swap" || event_type.starts_with("liquidity_"),
        "bridge" => event_type.starts_with("bridge_"),
        "lending" => event_type.starts_with("lending_"),
        "staking" => event_type.starts_with("staking_"),
        "mixer" => event_type.starts_with("mixer_"),
        "scam" => event_type == "scam_interaction",
        "system" => event_type == "bsc_system_transaction",
        _ => false,
    };
    valid
        .then_some(())
        .ok_or(RegistryValidationError::IncompatibleEvent)
}

fn stable_id(identity: &str) -> String {
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RegistryValidationError {
    #[error("invalid registry address in {0}")]
    InvalidAddress(&'static str),
    #[error("invalid registry network id")]
    InvalidNetwork,
    #[error("invalid or unsafe registry text in {0}")]
    InvalidText(&'static str),
    #[error("unsupported registry value for {field}")]
    UnsupportedChoice { field: &'static str },
    #[error("protocol type and decoder are incompatible")]
    IncompatibleDecoder,
    #[error("protocol type and mapped event are incompatible")]
    IncompatibleEvent,
    #[error("approved registry records require reviewed_by")]
    MissingReviewer,
    #[error("evidence_confidence must be finite and between zero and one")]
    InvalidConfidence,
    #[error("{0} mapping keys and values have different lengths")]
    MismatchedMappings(&'static str),
    #[error("registry mappings exceed the per-contract limit")]
    TooManyMappings,
    #[error("invalid hex key in {0} mapping")]
    InvalidMappingHex(&'static str),
    #[error("duplicate key in {0} mapping")]
    DuplicateMapping(&'static str),
    #[error("topic extraction index must be -1 or between zero and fifteen")]
    InvalidTopicIndex,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::{CanonicalLog, EvmU256};

    const HASH: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    const TX: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";
    const SENDER: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PROTOCOL: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const RECEIVER: &str = "0xcccccccccccccccccccccccccccccccccccccccc";
    const TOKEN_A: &str = "eip155:56/erc20:0xdddddddddddddddddddddddddddddddddddddddd";
    const TOKEN_B: &str = "eip155:56/erc20:0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn amount(value: &str) -> EvmU256 {
        EvmU256::parse("test.amount", value).unwrap()
    }

    fn transaction(status: u8, selector: &str) -> CanonicalTransaction {
        CanonicalTransaction {
            hash: TX.to_string(),
            transaction_index: 0,
            from_address: SENDER.to_string(),
            to_address: PROTOCOL.to_string(),
            contract_address: String::new(),
            nonce: 1,
            transaction_type: 2,
            value: amount("0x0"),
            input_selector: selector.to_string(),
            input_data: selector.to_string(),
            status,
            gas_limit: 100_000,
            gas_used: 50_000,
            effective_gas_price: amount("0x1"),
            fee_paid: amount("0xc350"),
        }
    }

    fn log(topic0: &str, topics: Vec<String>) -> CanonicalLog {
        CanonicalLog {
            event_id: format!("{BSC_NETWORK_ID}:{TX}:0"),
            tx_hash: TX.to_string(),
            transaction_index: 0,
            log_index: 0,
            contract_address: PROTOCOL.to_string(),
            topic0: topic0.to_string(),
            topics,
            data: "0x".to_string(),
        }
    }

    fn relationship(
        id: &str,
        from: &str,
        to: &str,
        asset: &str,
        value: &str,
        transfer_type: &'static str,
    ) -> CanonicalRelationship {
        CanonicalRelationship {
            relationship_id: id.to_string(),
            tx_hash: TX.to_string(),
            transaction_index: 0,
            event_index: 0,
            event_sub_index: 0,
            trace_address: Vec::new(),
            from_address: from.to_string(),
            to_address: to.to_string(),
            asset_id: asset.to_string(),
            token_id: String::new(),
            amount: amount(value),
            transfer_type,
        }
    }

    #[test]
    fn flow_sums_repeated_assets_without_counting_router_legs_twice() {
        let legs = vec![
            relationship("a", SENDER, "router", "asset", "0xa", "erc20"),
            relationship("b", "router", PROTOCOL, "asset", "0xa", "erc20"),
            relationship("c", SENDER, PROTOCOL, "asset", "0x5", "erc20"),
        ];
        let refs = legs.iter().collect::<Vec<_>>();
        let flow = subject_flow(SENDER, PROTOCOL, &refs);
        assert_eq!(flow.amounts_in, "15");
        assert_eq!(flow.evidence_refs, ["b", "c"]);
        assert_eq!(flow.boundary, "protocol");
        assert!(!flow.truncated);
    }

    #[test]
    fn flow_total_can_exceed_uint256_and_nfts_are_not_fungible_amounts() {
        let maximum = format!("0x{}", "f".repeat(64));
        let mut legs = vec![
            relationship("a", SENDER, PROTOCOL, "asset", &maximum, "erc20"),
            relationship("b", SENDER, PROTOCOL, "asset", &maximum, "erc20"),
            relationship("nft", SENDER, PROTOCOL, "nft-asset", "0x1", "erc1155"),
        ];
        legs[2].token_id = "12".into();
        let flow = subject_flow(SENDER, PROTOCOL, &legs.iter().collect::<Vec<_>>());
        let expected = U512::from_little_endian(&[255; 32]) * U512::from(2);
        assert_eq!(flow.amounts_in, expected.to_string());
        assert_eq!(flow.assets_in, "asset");
        assert_eq!(flow.nft_legs_excluded, 1);
        let many = vec![legs[0].clone(); 257];
        assert!(subject_flow(SENDER, PROTOCOL, &many.iter().collect::<Vec<_>>()).truncated);
    }

    fn block(transaction: CanonicalTransaction, logs: Vec<CanonicalLog>) -> CanonicalBlock {
        CanonicalBlock {
            number: 100,
            hash: HASH.to_string(),
            parent_hash: HASH.to_string(),
            timestamp_unix_ms: 1_700_000_000_000,
            transactions: vec![transaction],
            logs,
        }
    }

    fn extracted(relationships: Vec<CanonicalRelationship>) -> ExtractedBlockEvidence {
        ExtractedBlockEvidence {
            relationships,
            token_discoveries: Vec::new(),
            trace_data_complete: true,
        }
    }

    fn rule(protocol_type: &str, decoder: &str) -> ProtocolContract {
        ProtocolContract {
            network_id: BSC_NETWORK_ID.to_string(),
            contract_address: PROTOCOL.to_string(),
            protocol: "fixture_protocol".to_string(),
            protocol_type: protocol_type.to_string(),
            contract_role: "fixture".to_string(),
            decoder: decoder.to_string(),
            remote_network_id: "eip155:1".to_string(),
            remote_contract_address: RECEIVER.to_string(),
            method_ids: Vec::new(),
            method_event_types: Vec::new(),
            event_topics: Vec::new(),
            event_types: Vec::new(),
            remote_receiver_topic_index: -1,
            message_topic_index: -1,
            source_id: "fixture_source".to_string(),
            source_reference: "fixture://protocol".to_string(),
            review_status: "approved".to_string(),
            evidence_confidence: 1.0,
            enabled: 1,
            registry_revision: 7,
            reviewed_by: "fixture-reviewer".to_string(),
            review_note: String::new(),
            created_at_unix_ms: 1,
        }
    }

    fn registry(rule: ProtocolContract) -> SemanticRegistry {
        SemanticRegistry::from_contracts(vec![rule])
    }

    #[test]
    fn decodes_reviewed_amm_swap_and_liquidity_with_flow_evidence() {
        let input = extracted(vec![
            relationship("in", SENDER, PROTOCOL, TOKEN_A, "0x64", "erc20"),
            relationship("out", PROTOCOL, SENDER, TOKEN_B, "0x5a", "erc20"),
        ]);
        let block = block(
            transaction(1, "0x12345678"),
            vec![
                log(SWAP_V2_TOPIC, vec![SWAP_V2_TOPIC.to_string()]),
                CanonicalLog {
                    event_id: format!("{BSC_NETWORK_ID}:{TX}:1"),
                    log_index: 1,
                    topic0: V2_MINT_TOPIC.to_string(),
                    topics: vec![V2_MINT_TOPIC.to_string()],
                    ..log(V2_MINT_TOPIC, Vec::new())
                },
            ],
        );
        let result = classify_block(&registry(rule("dex", "amm_v2")), &block, &input);
        assert_eq!(result.events.len(), 2);
        assert!(result.events.iter().any(|event| event.event_type == "swap"));
        assert!(
            result
                .events
                .iter()
                .any(|event| event.event_type == "liquidity_add")
        );
        assert_eq!(result.transaction_features[0].is_swap, 1);
        assert_eq!(result.transaction_features[0].is_liquidity_add, 1);
        assert!(result.events.iter().all(|event| event.confidence <= 1.0));
    }

    #[test]
    fn bridge_topic_mapping_extracts_remote_receiver_and_message() {
        let bridge_topic = format!("0x{}", "1".repeat(64));
        let receiver_topic = format!("0x{}{}", "0".repeat(24), &RECEIVER[2..]);
        let message_topic = format!("0x{}", "2".repeat(64));
        let mut bridge = rule("bridge", "bridge_generic");
        bridge.event_topics = vec![bridge_topic.clone()];
        bridge.event_types = vec!["bridge_deposit".to_string()];
        bridge.remote_receiver_topic_index = 1;
        bridge.message_topic_index = 2;
        let input = extracted(vec![relationship(
            "bridge-in",
            SENDER,
            PROTOCOL,
            TOKEN_A,
            "0x64",
            "erc20",
        )]);
        let block = block(
            transaction(1, "0x12345678"),
            vec![log(
                &bridge_topic,
                vec![bridge_topic.clone(), receiver_topic, message_topic.clone()],
            )],
        );
        let first = classify_block(&registry(bridge.clone()), &block, &input);
        let second = classify_block(&registry(bridge), &block, &input);
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].event_type, "bridge_deposit");
        assert_eq!(first.events[0].remote_network_id, "eip155:1");
        assert_eq!(first.events[0].remote_receiver, RECEIVER);
        assert_eq!(first.events[0].bridge_message_id, message_topic);
        assert_eq!(first.events[0].event_id, second.events[0].event_id);
    }

    #[test]
    fn every_configurable_semantic_type_has_a_positive_method_fixture() {
        let cases = [
            ("dex", "amm_v2", "swap"),
            ("dex", "amm_v2", "liquidity_remove"),
            ("bridge", "bridge_generic", "bridge_withdrawal"),
            ("lending", "lending_generic", "lending_supply"),
            ("lending", "lending_generic", "lending_withdrawal"),
            ("lending", "lending_generic", "lending_borrow"),
            ("lending", "lending_generic", "lending_repay"),
            ("lending", "lending_generic", "lending_liquidation"),
            ("staking", "staking_generic", "staking_deposit"),
            ("staking", "staking_generic", "staking_withdrawal"),
            ("staking", "staking_generic", "staking_claim"),
            ("mixer", "mixer_generic", "mixer_deposit"),
            ("mixer", "mixer_generic", "mixer_withdrawal"),
            ("scam", "reviewed_interaction", "scam_interaction"),
            ("system", "bsc_system", "bsc_system_transaction"),
        ];
        for (protocol_type, decoder, event_type) in cases {
            let mut record = rule(protocol_type, decoder);
            record.method_ids = vec!["0x12345678".to_string()];
            record.method_event_types = vec![event_type.to_string()];
            let block = block(transaction(1, "0x12345678"), Vec::new());
            let result = classify_block(&registry(record), &block, &extracted(Vec::new()));
            assert!(
                result
                    .events
                    .iter()
                    .any(|event| event.event_type == event_type),
                "missing fixture output for {event_type}"
            );
        }
    }

    #[test]
    fn zero_address_transfers_produce_mint_and_burn_without_registry() {
        let input = extracted(vec![
            relationship("mint", ZERO_ADDRESS, SENDER, TOKEN_A, "0x2", "erc20"),
            relationship("burn", SENDER, ZERO_ADDRESS, TOKEN_A, "0x1", "erc20"),
        ]);
        let result = classify_block(
            &SemanticRegistry::default(),
            &block(transaction(1, "0x"), Vec::new()),
            &input,
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| event.event_type == "token_mint")
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| event.event_type == "token_burn")
        );
        assert_eq!(result.transaction_features[0].is_mint, 1);
        assert_eq!(result.transaction_features[0].is_burn, 1);
    }

    #[test]
    fn unreviewed_unregistered_and_failed_transactions_emit_nothing() {
        let standard_log = log(SWAP_V2_TOPIC, vec![SWAP_V2_TOPIC.to_string()]);
        let active_block = block(transaction(1, "0x12345678"), vec![standard_log.clone()]);
        assert!(
            classify_block(
                &SemanticRegistry::default(),
                &active_block,
                &extracted(Vec::new())
            )
            .events
            .is_empty()
        );
        let mut pending = rule("dex", "amm_v2");
        pending.review_status = "pending".to_string();
        assert!(
            classify_block(&registry(pending), &active_block, &extracted(Vec::new()))
                .events
                .is_empty()
        );
        let failed_block = block(transaction(0, "0x12345678"), vec![standard_log]);
        assert!(
            classify_block(
                &registry(rule("dex", "amm_v2")),
                &failed_block,
                &extracted(Vec::new())
            )
            .events
            .is_empty()
        );
    }

    #[test]
    fn registry_validation_rejects_unreviewed_claims_and_incompatible_mappings() {
        let input = ProtocolRegistryInput {
            contract_address: PROTOCOL.to_string(),
            protocol: "fixture".to_string(),
            protocol_type: "bridge".to_string(),
            contract_role: "router".to_string(),
            decoder: "amm_v2".to_string(),
            remote_network_id: String::new(),
            remote_contract_address: String::new(),
            method_ids: Vec::new(),
            method_event_types: Vec::new(),
            event_topics: Vec::new(),
            event_types: Vec::new(),
            remote_receiver_topic_index: -1,
            message_topic_index: -1,
            source_id: "fixture".to_string(),
            source_reference: String::new(),
            review_status: "approved".to_string(),
            evidence_confidence: 1.0,
            enabled: true,
            reviewed_by: String::new(),
            review_note: String::new(),
        };
        assert_eq!(
            input.validate_only(),
            Err(RegistryValidationError::IncompatibleDecoder)
        );
        let mut input = input;
        input.decoder = "bridge_generic".to_string();
        assert_eq!(
            input.validate_only(),
            Err(RegistryValidationError::MissingReviewer)
        );
    }
}
