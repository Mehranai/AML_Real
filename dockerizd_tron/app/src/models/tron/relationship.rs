use clickhouse::Row;
use clickhouse::types::UInt256;
use serde::Serialize;

#[derive(Debug, Clone, Row, Serialize)]
pub struct AddressRelationshipRow {
    pub relationship_id: String,

    pub from_address: String,
    pub to_address: String,
    pub token_address: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub amount: UInt256,
    pub transfer_type: String,
}
