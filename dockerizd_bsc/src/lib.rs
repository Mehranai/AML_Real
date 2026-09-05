pub mod config;
pub mod db;
pub mod domain;
pub mod ingestion;
pub mod rpc;
pub mod runtime;
pub mod semantic;

pub const BSC_CHAIN_ID: u64 = 56;
pub const BSC_NETWORK_ID: &str = "eip155:56";
pub const BSC_CLICKHOUSE_DATABASE: &str = "bsc_aml";
