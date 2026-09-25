use std::time::Duration;

use ethereum_aml::{
    clustering::discover_address_clusters,
    config::AppConfig,
    db::initialize_ethereum_schema,
    exposure::{ExposureOptions, propagate_exposure},
    init_tracing,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;

    loop {
        let clustering = discover_address_clusters(&config).await;
        match &clustering {
            Ok(report) => tracing::info!(
                run_id = %report.run_id,
                entity_memberships = report.entity_memberships,
                control_claims = report.control_claims,
                "Ethereum clustering run completed"
            ),
            Err(error) => tracing::error!(error = %error, "Ethereum clustering run failed"),
        }

        let options = ExposureOptions {
            max_hops: config.eth_exposure_max_hops,
            hop_decay: config.eth_exposure_hop_decay,
            time_half_life_days: config.eth_exposure_time_half_life_days,
            max_paths_per_subject: config.eth_exposure_max_paths_per_subject,
        };
        let exposure = propagate_exposure(&config, options).await;
        match &exposure {
            Ok(report) if report.seed_count == 0 => {
                tracing::warn!(run_id = %report.run_id, "Ethereum exposure not assessed: no active risk seeds")
            }
            Ok(report) => tracing::info!(
                run_id = %report.run_id,
                seeds = report.seed_count,
                paths = report.path_count,
                "Ethereum exposure propagation completed"
            ),
            Err(error) => tracing::warn!(
                error = %error,
                "Ethereum exposure propagation skipped or failed"
            ),
        }

        if clustering.is_err() || exposure.is_err() {
            // Exit visibly for restart/monitoring, without a tight retry loop.
            tokio::time::sleep(Duration::from_secs(30)).await;
            clustering?;
            exposure?;
        }

        tokio::time::sleep(Duration::from_secs(config.eth_analytics_interval_seconds)).await;
    }
}
