pub mod api;
pub mod bitcoin;
pub mod config;
pub mod db;
pub mod domain;
pub mod storage;
// Preserve the existing library API while the implementation follows the
// domain/db/storage boundaries used by the other chain adapters.
pub use bitcoin::{failures, fetch, ingestion, probe, repair, resolver, rpc};
pub use db::warehouse;
pub use domain::model;
pub use storage::writer_lock;

pub const NETWORK_ID: &str = "bip122:000000000019d6689c085ae165831e93";
pub const GENESIS: &str = "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f";
