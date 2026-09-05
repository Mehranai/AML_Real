use anyhow::Result;

use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::services::tron::tron_metadata_worker::run_token_metadata_worker;

#[tokio::main]
async fn main() -> Result<()> {
    run_token_metadata_worker(AppConfig::from_env()).await
}
