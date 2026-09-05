use std::sync::Arc;

use anyhow::{Result, anyhow};
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::services::tron::entity_intelligence::{
    IntelligenceReviewInput, review_intelligence_subject,
};
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 4 {
        return Err(anyhow!(
            "usage: tron_review_intelligence <ENTITY_LABEL|CLUSTER_CLAIM> <subject_id> <APPROVED|REJECTED> <reviewer> [reason]"
        ));
    }
    let config = AppConfig::from_env();
    let clickhouse = Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron),
    );
    let row = review_intelligence_subject(
        clickhouse,
        IntelligenceReviewInput {
            subject_type: args[0].clone(),
            subject_id: args[1].clone(),
            decision: args[2].clone(),
            reviewer: args[3].clone(),
            reason: args.get(4).cloned().unwrap_or_default(),
            evidence_refs: Vec::new(),
        },
    )
    .await?;

    println!(
        "[TRON INTELLIGENCE REVIEW] review_id={} subject_type={} subject_id={} decision={}",
        row.review_id, row.subject_type, row.subject_id, row.decision
    );
    Ok(())
}
