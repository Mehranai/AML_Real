use clickhouse::Row;
use clickhouse::types::UInt256;
use serde::{Deserialize, Serialize};

//
// ======================================================
// TRANSACTIONS
// ======================================================
//

#[derive(Debug, Row, Serialize, Clone)]
pub struct TransactionRow {
    pub tx_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub initiator_address: String,
    pub target_address: String,
    pub contract_address: String,
    pub contract_type: String,
    pub fee: UInt256,
    pub energy_usage_total: u64,
    pub net_usage: u64,
    pub status: u8,
}

//
// ======================================================
// SEMANTIC AML EVENTS
// ======================================================
//

#[derive(Debug, Row, Serialize, Clone)]
pub struct SemanticAmlEventRow {
    pub event_id: String,
    pub chain: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub event_type: String,
    pub subject_address: String,
    pub protocol: String,
    pub asset_in: String,
    pub asset_out: String,
    pub detector: String,
    pub detector_version: String,
    pub confidence: f32,
    pub evidence_json: String,
}

#[derive(Debug, Row, Serialize, Clone)]
pub struct IngestedBlockRow {
    pub chain: String,
    pub block_number: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub block_timestamp: u64,
    pub transaction_count: u32,
    pub finality_status: String,
    pub ingestion_status: String,
    pub error_message: String,
    pub indexed_at_unix_ms: u64,
}

#[derive(Debug, Row, Serialize, Deserialize, Clone)]
pub struct IngestionFailureRow {
    pub failure_id: String,
    pub chain: String,
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

#[derive(Debug, Row, Serialize, Clone)]
pub struct IngestionBenchmarkRow {
    pub run_id: String,
    pub chain: String,
    pub source_kind: String,
    pub start_block: u64,
    pub end_block: u64,
    pub requested_blocks: u32,
    pub completed_blocks: u32,
    pub transaction_count: u64,
    pub elapsed_ms: u64,
    pub blocks_per_second: f64,
    pub transactions_per_second: f64,
    pub rows_before: u64,
    pub rows_after: u64,
    pub compressed_bytes_before: u64,
    pub compressed_bytes_after: u64,
    pub investigation_address: String,
    pub investigation_latency_ms: u64,
    pub status: String,
    pub error_message: String,
    pub metrics_json: String,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
}

#[derive(Debug, Row, Serialize, Clone)]
pub struct TokenMetadataDiscoveryRow {
    pub token_address: String,
    pub discovered_block: u64,
    pub discovered_at_unix_ms: u64,
}

//
// ======================================================
// TRANSACTION FEATURES
// ======================================================
//

#[derive(Debug, Row, Serialize, Clone)]
pub struct TransactionFeatureRow {
    pub tx_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub transaction_type: String,
    pub transaction_subtype: String,
    pub classification_confidence: f32,
    pub classification_source: String,
    pub protocol: String,
    pub method_id: String,
    pub is_swap: u8,
    pub is_bridge: u8,
    pub is_mint: u8,
    pub is_burn: u8,
    pub is_liquidity_add: u8,
    pub is_liquidity_remove: u8,
    pub is_contract_call: u8,
}
