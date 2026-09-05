use clickhouse::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct TokenMetadataRow {
    pub token_address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub is_verified: u8,
}
