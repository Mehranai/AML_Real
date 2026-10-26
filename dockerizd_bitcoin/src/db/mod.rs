//! Bitcoin database types and module boundaries, matching the chain adapters.
mod client;
pub mod migrations;
mod queries;
mod sql;
pub mod warehouse;
pub use sql::{baseline_sql, schema_sql};

use crate::config::DatabaseConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct Database {
    client: reqwest::Client,
    config: Arc<DatabaseConfig>,
}
pub(crate) struct SharedWarehouseConnection<'a>(&'a Database);
#[derive(Clone, Default, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct SyncState {
    pub id: u8,
    pub revision: u64,
    pub initialized: u8,
    pub start_height: u64,
    pub tip_height: u64,
    pub tip_hash: String,
    pub observed_rpc_height: u64,
    pub published_at: u64,
}
pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}
pub fn next_revision(previous: u64) -> u64 {
    (timestamp() * 1000).max(previous + 1)
}
