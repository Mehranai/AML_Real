use clickhouse::Row;
use serde::Serialize;

#[derive(Debug, Clone, Row, Serialize)]
pub struct ExposureSeedRow {
    pub address: String,
    pub entity_name: String,
    pub entity_type: String,
    pub risk_level: u8,
    pub source: String,
    pub source_label_id: String,
    pub is_active: u8,
}

#[derive(Debug, Clone, Row, Serialize)]
pub struct AddressExposureRow {
    pub source_address: String,
    pub exposed_address: String,
    pub hop_distance: u8,
    pub exposure_score: f64,
    pub path_count: u32,
    pub last_tx_hash: String,
    pub last_seen_block: u64,
    pub exposure_type: String,
    pub best_path_amount_share: f64,
    pub best_path_time_weight: f64,
    pub service_mediated: u8,
    pub propagation_run_id: String,
}
