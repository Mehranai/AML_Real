pub mod api;
pub mod api_security;
pub mod clustering;
pub mod config;
pub mod db;
pub mod domain;
pub mod ethereum;
pub mod exposure;
pub mod graph;
pub mod intelligence;
pub mod investigation;
pub mod risk;
pub mod storage;

pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
