use clickhouse::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct IntelligenceSourceRow {
    pub chain: String,
    pub source_id: String,
    pub source_name: String,
    pub source_type: String,
    pub trust_tier: String,
    pub reference_url: String,
    pub license: String,
    pub is_active: u8,
    pub created_by: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct EntityLabelClaimRow {
    pub label_id: String,
    pub chain: String,
    pub address: String,
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub risk_percent: u8,
    pub source: String,
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
    pub chain: String,
    pub subject_type: String,
    pub subject_id: String,
    pub decision: String,
    pub reviewer: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct AddressClusterClaimRow {
    pub claim_id: String,
    pub chain: String,
    pub address: String,
    pub cluster_id: String,
    pub cluster_type: String,
    pub address_role: String,
    pub claim_method: String,
    pub confidence: f32,
    pub source: String,
    pub source_record_id: String,
    pub evidence_tx_hashes: Vec<String>,
    pub evidence_addresses: Vec<String>,
    pub evidence_json: String,
    pub supersedes_claim_id: String,
    pub review_status: String,
    pub created_by: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct AddressClusterMembershipRow {
    pub chain: String,
    pub address: String,
    pub cluster_id: String,
    pub cluster_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub source_claim_id: String,
    pub review_id: String,
    pub cluster_version: u32,
    pub is_active: u8,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct ClusterVersionRow {
    pub chain: String,
    pub cluster_id: String,
    pub version: u32,
    pub cluster_type: String,
    pub display_name: String,
    pub change_type: String,
    pub change_reason: String,
    pub source_claim_ids: Vec<String>,
    pub active_member_count: u64,
    pub created_by: String,
    pub created_at_unix_ms: u64,
}
