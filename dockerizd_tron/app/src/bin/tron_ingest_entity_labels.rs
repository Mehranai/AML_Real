use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::services::tron::entity_intelligence::{
    ENTITY_LABEL_SUBJECT, EntityLabelSubmission, IntelligenceReviewInput,
    review_intelligence_subject, submit_entity_label,
};
use clickhouse::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct EntityLabelFileRecord {
    #[serde(flatten)]
    submission: EntityLabelSubmission,
    #[serde(default)]
    review_status: String,
    #[serde(default)]
    reviewed_by: String,
    #[serde(default)]
    review_reason: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: tron_ingest_entity_labels <labels.jsonl>"))?;
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read entity label file {path}"))?;
    let config = AppConfig::from_env();
    let clickhouse = Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron),
    );
    let mut submitted = 0usize;
    let mut reviewed = 0usize;

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: EntityLabelFileRecord = serde_json::from_str(line)
            .with_context(|| format!("invalid JSON on line {}", line_index + 1))?;

        // اینجا فایل json به این تابع داده میشود که بررسی شود آیا exchange هست یا نه
        let claim = submit_entity_label(&clickhouse, record.submission)
            .await
            .with_context(|| format!("failed to submit label on line {}", line_index + 1))?;
        submitted += 1;

        let decision = record.review_status.trim().to_ascii_uppercase();
        if decision.is_empty() || decision == "PENDING" {
            continue;
        }
        if record.reviewed_by.trim().is_empty() {
            return Err(anyhow!(
                "reviewed_by is required for {} label on line {}",
                decision,
                line_index + 1
            ));
        }

        // اینجا هم بررسی میشه اگه توی 4 تا حالت بود یعنی exchange هستش
        // پس اینجا تصمیم گرفته میشود وارد exchange_addresses شود
        review_intelligence_subject(
            clickhouse.clone(),
            IntelligenceReviewInput {
                subject_type: ENTITY_LABEL_SUBJECT.to_string(),
                subject_id: claim.label_id,
                decision,
                reviewer: record.reviewed_by,
                reason: record.review_reason,
                evidence_refs: Vec::new(),
            },
        )
        .await
        .with_context(|| format!("failed to review label on line {}", line_index + 1))?;
        reviewed += 1;
    }

    println!(
        "[TRON ENTITY INTELLIGENCE] submitted={} reviewed={} source={}",
        submitted, reviewed, path
    );
    Ok(())
}
