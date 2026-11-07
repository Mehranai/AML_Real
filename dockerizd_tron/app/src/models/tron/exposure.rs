use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
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
    pub best_path: ExposurePathEvidence,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ExposurePathEvidence {
    pub subjects: Vec<String>,
    pub relationship_ids: Vec<String>,
    pub tx_hashes: Vec<String>,
    pub asset_id: String,
    pub amount_raw: String,
    pub first_transfer_unix_ms: u64,
    pub last_transfer_unix_ms: u64,
}
