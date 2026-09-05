use std::{
    path::Path,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use clickhouse::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    config::AppConfig,
    db::database_client,
    domain::{AddressId, NetworkId},
    storage::{AddressEntityRow, EntityLabelRow, IntelligenceReviewRow, IntelligenceSourceRow},
};

#[derive(Debug, Clone)]
pub struct IntelligenceSourceRegistration {
    pub source_id: String,
    pub source_name: String,
    pub source_type: String,
    pub trust_tier: String,
    pub reference_url: String,
    pub created_by: String,
}

#[derive(Debug, Clone)]
pub struct EntityLabelImportOptions {
    pub source: IntelligenceSourceRegistration,
    pub submitted_by: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EntityLabelImportReport {
    pub source_id: String,
    pub submitted: u64,
    pub approved: u64,
    pub rejected: u64,
    pub pending: u64,
}

#[derive(Debug, Deserialize)]
struct EntityLabelCsvRow {
    address: String,
    #[serde(default)]
    entity_id: String,
    entity_name: String,
    entity_type: String,
    #[serde(default = "default_address_role")]
    address_role: String,
    confidence: f32,
    #[serde(default)]
    risk_level: u8,
    #[serde(default)]
    is_exposure_seed: bool,
    #[serde(default)]
    seed_category: String,
    source_record_id: String,
    #[serde(default)]
    supersedes_label_id: String,
    #[serde(default)]
    case_id: String,
    evidence_refs: String,
    #[serde(default = "default_review_status")]
    review_status: String,
    #[serde(default)]
    reviewed_by: String,
    #[serde(default)]
    review_reason: String,
}

pub async fn import_entity_labels_csv(
    config: &AppConfig,
    path: &Path,
    options: EntityLabelImportOptions,
) -> anyhow::Result<EntityLabelImportReport> {
    validate_source(&options.source)?;
    ensure!(
        !options.submitted_by.trim().is_empty(),
        "submitted_by must not be empty"
    );

    let client = database_client(config);
    let now = unix_time_millis()?;
    insert_one(
        &client,
        "intelligence_sources",
        &IntelligenceSourceRow {
            network_id: config.eth_network_id.clone(),
            source_id: options.source.source_id.clone(),
            source_name: options.source.source_name.trim().to_string(),
            source_type: options.source.source_type.trim().to_ascii_uppercase(),
            trust_tier: options.source.trust_tier.trim().to_ascii_uppercase(),
            reference_url: options.source.reference_url.trim().to_string(),
            is_active: 1,
            created_by: options.source.created_by.trim().to_string(),
            created_at_unix_ms: now,
        },
    )
    .await?;

    let network = NetworkId::from_str(&config.eth_network_id)?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(false)
        .from_path(path)
        .with_context(|| format!("failed to open entity-label CSV {}", path.display()))?;
    let mut report = EntityLabelImportReport {
        source_id: options.source.source_id.clone(),
        submitted: 0,
        approved: 0,
        rejected: 0,
        pending: 0,
    };

    for (index, record) in reader.deserialize::<EntityLabelCsvRow>().enumerate() {
        let record = record.with_context(|| format!("invalid CSV row {}", index + 2))?;
        let address = AddressId::parse_evm(network.clone(), &record.address)
            .with_context(|| format!("invalid address on CSV row {}", index + 2))?
            .address()
            .to_string();
        ensure!(
            !record.entity_name.trim().is_empty(),
            "entity_name is required on CSV row {}",
            index + 2
        );
        ensure!(
            !record.entity_type.trim().is_empty(),
            "entity_type is required on CSV row {}",
            index + 2
        );
        ensure!(
            !record.source_record_id.trim().is_empty(),
            "source_record_id is required on CSV row {}",
            index + 2
        );
        ensure!(
            (0.0..=1.0).contains(&record.confidence),
            "confidence must be between 0 and 1 on CSV row {}",
            index + 2
        );
        ensure!(
            record.risk_level <= 100,
            "risk_level must be between 0 and 100 on CSV row {}",
            index + 2
        );

        if record.is_exposure_seed {
            ensure!(
                record.risk_level > 0,
                "an exposure seed must have risk_level greater than 0 on CSV row {}",
                index + 2
            );
            ensure!(
                !record.seed_category.trim().is_empty(),
                "seed_category is required for an exposure seed on CSV row {}",
                index + 2
            );
        }

        let evidence_refs = record
            .evidence_refs
            .split('|')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        ensure!(
            !evidence_refs.is_empty(),
            "evidence_refs must contain at least one reference on CSV row {}",
            index + 2
        );

        let entity_type = record.entity_type.trim().to_ascii_lowercase();
        let entity_id = if record.entity_id.trim().is_empty() {
            format!(
                "eth:{}:{}",
                slug(&entity_type),
                slug(record.entity_name.trim())
            )
        } else {
            record.entity_id.trim().to_string()
        };
        let address_role = normalize_role(&record.address_role)?;
        let review_status = normalize_review_status(&record.review_status)?;
        if review_status != "PENDING" {
            ensure!(
                !record.reviewed_by.trim().is_empty(),
                "reviewed_by is required for {} row {}",
                review_status,
                index + 2
            );
        }
        let created_at_unix_ms = unix_time_millis()?;
        let label_id = stable_id(
            "eth_label",
            &[
                &config.eth_network_id,
                &address,
                &entity_id,
                &address_role,
                &options.source.source_id,
                record.source_record_id.trim(),
            ],
        );
        let label = EntityLabelRow {
            label_id: label_id.clone(),
            network_id: config.eth_network_id.clone(),
            address: address.clone(),
            entity_id: entity_id.clone(),
            entity_name: record.entity_name.trim().to_string(),
            entity_type: entity_type.clone(),
            address_role: address_role.clone(),
            confidence: record.confidence,
            risk_level: record.risk_level,
            is_exposure_seed: u8::from(record.is_exposure_seed),
            seed_category: record.seed_category.trim().to_ascii_uppercase(),
            source_id: options.source.source_id.clone(),
            source_record_id: record.source_record_id.trim().to_string(),
            supersedes_label_id: record.supersedes_label_id.trim().to_string(),
            submitted_by: options.submitted_by.trim().to_string(),
            case_id: record.case_id.trim().to_string(),
            evidence_refs: evidence_refs.clone(),
            review_status: review_status.clone(),
            created_at_unix_ms,
        };
        insert_one(&client, "entity_labels", &label).await?;
        report.submitted += 1;

        match review_status.as_str() {
            "APPROVED" => {
                let review_id = insert_review(
                    &client,
                    config,
                    &label_id,
                    "APPROVED",
                    &record.reviewed_by,
                    &record.review_reason,
                    &evidence_refs,
                    created_at_unix_ms,
                )
                .await?;
                insert_one(
                    &client,
                    "address_entities",
                    &AddressEntityRow {
                        network_id: config.eth_network_id.clone(),
                        address,
                        entity_id,
                        entity_name: label.entity_name,
                        entity_type,
                        address_role,
                        confidence: record.confidence,
                        risk_level: record.risk_level,
                        is_exposure_seed: u8::from(record.is_exposure_seed),
                        seed_category: record.seed_category.trim().to_ascii_uppercase(),
                        source_label_id: label_id,
                        review_id,
                        is_active: 1,
                        created_at_unix_ms,
                    },
                )
                .await?;
                report.approved += 1;
            }
            "REJECTED" => {
                insert_review(
                    &client,
                    config,
                    &label_id,
                    "REJECTED",
                    &record.reviewed_by,
                    &record.review_reason,
                    &evidence_refs,
                    created_at_unix_ms,
                )
                .await?;
                report.rejected += 1;
            }
            _ => report.pending += 1,
        }
    }

    Ok(report)
}

async fn insert_review(
    client: &Client,
    config: &AppConfig,
    label_id: &str,
    decision: &str,
    reviewer: &str,
    reason: &str,
    evidence_refs: &[String],
    created_at_unix_ms: u64,
) -> anyhow::Result<String> {
    let review_id = stable_id(
        "eth_review",
        &[
            label_id,
            decision,
            reviewer.trim(),
            &created_at_unix_ms.to_string(),
        ],
    );
    insert_one(
        client,
        "intelligence_reviews",
        &IntelligenceReviewRow {
            review_id: review_id.clone(),
            network_id: config.eth_network_id.clone(),
            subject_type: "ENTITY_LABEL".to_string(),
            subject_id: label_id.to_string(),
            decision: decision.to_string(),
            reviewer: reviewer.trim().to_string(),
            reason: reason.trim().to_string(),
            evidence_refs: evidence_refs.to_vec(),
            created_at_unix_ms,
        },
    )
    .await?;
    Ok(review_id)
}

fn validate_source(source: &IntelligenceSourceRegistration) -> anyhow::Result<()> {
    ensure!(
        !source.source_id.trim().is_empty(),
        "source_id must not be empty"
    );
    ensure!(
        source
            .source_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-')),
        "source_id contains unsupported characters"
    );
    ensure!(
        !source.source_name.trim().is_empty(),
        "source_name must not be empty"
    );
    ensure!(
        !source.created_by.trim().is_empty(),
        "created_by must not be empty"
    );
    let source_type = source.source_type.trim().to_ascii_uppercase();
    ensure!(
        matches!(
            source_type.as_str(),
            "ANALYST"
                | "LAW_ENFORCEMENT"
                | "REGULATORY"
                | "VENDOR"
                | "PUBLIC_RESEARCH"
                | "INTERNAL"
        ),
        "unsupported source_type"
    );
    let trust_tier = source.trust_tier.trim().to_ascii_uppercase();
    ensure!(
        matches!(
            trust_tier.as_str(),
            "VERIFIED" | "HIGH" | "MEDIUM" | "UNVERIFIED"
        ),
        "unsupported trust_tier"
    );
    Ok(())
}

fn normalize_role(value: &str) -> anyhow::Result<String> {
    let role = value.trim().to_ascii_uppercase();
    ensure!(
        matches!(
            role.as_str(),
            "UNKNOWN"
                | "DEPOSIT"
                | "HOT_WALLET"
                | "COLD_WALLET"
                | "TREASURY"
                | "CONTRACT"
                | "ROUTER"
                | "POOL"
                | "BRIDGE"
                | "MIXER"
        ),
        "unsupported address_role {role}"
    );
    Ok(role)
}

fn normalize_review_status(value: &str) -> anyhow::Result<String> {
    let status = value.trim().to_ascii_uppercase();
    ensure!(
        matches!(status.as_str(), "PENDING" | "APPROVED" | "REJECTED"),
        "unsupported review_status {status}"
    );
    Ok(status)
}

async fn insert_one<R>(client: &Client, table: &str, row: &R) -> anyhow::Result<()>
where
    R: clickhouse::RowOwned + clickhouse::RowWrite,
{
    let mut insert = client
        .insert::<R>(table)
        .await
        .with_context(|| format!("failed to start insert into {table}"))?;
    insert.write(row).await?;
    insert.end().await?;
    Ok(())
}

fn stable_id(namespace: &str, fields: &[&str]) -> String {
    let mut input = namespace.to_string();
    for field in fields {
        input.push('|');
        input.push_str(field);
    }
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn slug(value: &str) -> String {
    let value = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    value.trim_matches('_').to_string()
}

fn unix_time_millis() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis()
        .try_into()
        .context("Unix timestamp does not fit UInt64")?)
}

fn default_address_role() -> String {
    "UNKNOWN".to_string()
}

fn default_review_status() -> String {
    "PENDING".to_string()
}

#[cfg(test)]
mod tests {
    use super::{normalize_review_status, normalize_role, slug};

    #[test]
    fn normalizes_reviewed_intelligence_enums() {
        assert_eq!(normalize_role("hot_wallet").unwrap(), "HOT_WALLET");
        assert_eq!(normalize_review_status("approved").unwrap(), "APPROVED");
        assert!(normalize_review_status("trusted").is_err());
    }

    #[test]
    fn creates_stable_entity_slugs() {
        assert_eq!(slug("Example Exchange"), "example_exchange");
    }
}
