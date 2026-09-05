use clickhouse::{Row, types::UInt256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct IngestedBlockRow {
    pub network_id: String,
    pub block_number: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub block_timestamp_unix_ms: u64,
    pub transaction_count: u32,
    pub ingestion_status: String,
    pub receipt_data_complete: u8,
    pub trace_data_complete: u8,
    pub rpc_provider: String,
    pub error_message: String,
    pub indexed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TransactionRow {
    pub network_id: String,
    pub tx_hash: String,
    pub block_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub transaction_index: u32,
    pub from_address: String,
    pub to_address: String,
    pub contract_address: String,
    pub nonce: u64,
    pub transaction_type: u8,
    pub value: UInt256,
    pub input_selector: String,
    pub input_data: String,
    pub status: u8,
    pub status_known: u8,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub effective_gas_price: UInt256,
    pub fee_paid: UInt256,
    pub max_fee_per_gas: UInt256,
    pub max_priority_fee_per_gas: UInt256,
    pub blob_gas_used: u64,
    pub blob_gas_price: UInt256,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct EvmLogRow {
    pub event_id: String,
    pub network_id: String,
    pub block_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub tx_hash: String,
    pub transaction_index: u32,
    pub log_index: u32,
    pub contract_address: String,
    pub topic0: String,
    pub topics: Vec<String>,
    pub data: String,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct AddressRelationshipRow {
    pub relationship_id: String,
    pub network_id: String,
    pub block_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub tx_hash: String,
    pub transaction_index: u32,
    pub event_index: u32,
    pub event_sub_index: u32,
    pub trace_address: Vec<u32>,
    pub from_address: String,
    pub to_address: String,
    pub asset_id: String,
    pub token_id: String,
    pub amount: UInt256,
    pub transfer_type: String,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TransactionFeatureRow {
    pub feature_id: String,
    pub network_id: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub transaction_type: String,
    pub transaction_subtype: String,
    pub protocol: String,
    pub method_id: String,
    pub is_swap: u8,
    pub is_bridge: u8,
    pub is_mixer: u8,
    pub is_mint: u8,
    pub is_burn: u8,
    pub is_liquidity_add: u8,
    pub is_liquidity_remove: u8,
    pub is_contract_call: u8,
    pub unique_assets: u16,
    pub participants: u16,
    pub classification_confidence: f32,
    pub detector: String,
    pub detector_version: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct SemanticAmlEventRow {
    pub event_id: String,
    pub network_id: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub event_index: u32,
    pub event_type: String,
    pub subject_address: String,
    pub protocol: String,
    pub protocol_contract: String,
    pub counterparty_address: String,
    pub asset_in: String,
    pub asset_out: String,
    pub remote_network_id: String,
    pub remote_asset: String,
    pub bridge_direction: String,
    pub correlation_key: String,
    pub amount_in: String,
    pub amount_out: String,
    pub detector: String,
    pub detector_version: String,
    pub confidence: f32,
    pub evidence_json: String,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct ProtocolContractRow {
    pub network_id: String,
    pub contract_address: String,
    pub protocol: String,
    pub protocol_type: String,
    pub contract_role: String,
    pub remote_network_id: String,
    pub remote_contract_address: String,
    pub decoder: String,
    pub source: String,
    pub confidence: f32,
    pub enabled: u8,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TokenMetadataRow {
    pub network_id: String,
    pub token_address: String,
    pub token_standard: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub decimals_known: u8,
    pub total_supply: String,
    pub code_hash: String,
    pub is_verified: u8,
    pub metadata_source: String,
    pub metadata_status: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct IntelligenceSourceRow {
    pub network_id: String,
    pub source_id: String,
    pub source_name: String,
    pub source_type: String,
    pub trust_tier: String,
    pub reference_url: String,
    pub is_active: u8,
    pub created_by: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct EntityLabelRow {
    pub label_id: String,
    pub network_id: String,
    pub address: String,
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub risk_level: u8,
    pub is_exposure_seed: u8,
    pub seed_category: String,
    pub source_id: String,
    pub source_record_id: String,
    pub supersedes_label_id: String,
    pub submitted_by: String,
    pub case_id: String,
    pub evidence_refs: Vec<String>,
    pub review_status: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct IntelligenceReviewRow {
    pub review_id: String,
    pub network_id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub decision: String,
    pub reviewer: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct AddressEntityRow {
    pub network_id: String,
    pub address: String,
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub risk_level: u8,
    pub is_exposure_seed: u8,
    pub seed_category: String,
    pub source_label_id: String,
    pub review_id: String,
    pub is_active: u8,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TokenMetadataDiscoveryRow {
    pub network_id: String,
    pub token_address: String,
    pub token_standard: String,
    pub discovered_block: u64,
    pub discovered_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TokenMetadataJobRow {
    pub network_id: String,
    pub token_address: String,
    pub token_standard: String,
    pub discovered_block: u64,
    pub status: String,
    pub attempt_count: u8,
    pub last_error: String,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct IngestionFailureRow {
    pub failure_id: String,
    pub network_id: String,
    pub block_number: u64,
    pub block_hash: String,
    pub tx_hash: String,
    pub stage: String,
    pub error_class: String,
    pub error_message: String,
    pub retryable: u8,
    pub attempt_count: u32,
    pub status: String,
    pub first_failed_at_unix_ms: u64,
    pub last_failed_at_unix_ms: u64,
    pub resolved_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct SyncStateRow {
    pub network_id: String,
    pub last_synced_block: u64,
    pub last_synced_block_hash: String,
    pub updated_at_unix_ms: u64,
}
