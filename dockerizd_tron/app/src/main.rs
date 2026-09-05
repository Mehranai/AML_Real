use anyhow::Result;

use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::tasks::fetch_loop::run_tron_loop;

// نقطه شروع برنامه
#[tokio::main]
async fn main() -> Result<()> {
    run_tron_loop(AppConfig::from_env()).await
}
