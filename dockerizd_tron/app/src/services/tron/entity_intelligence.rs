use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use clickhouse::Client;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::tron::exchange::{AddressEntityRow, ExchangeAddressRow};
use crate::models::tron::exposure::ExposureSeedRow;
use crate::models::tron::intelligence::{
    AddressClusterClaimRow, AddressClusterMembershipRow, ClusterVersionRow, EntityLabelClaimRow,
    IntelligenceReviewRow, IntelligenceSourceRow,
};
use crate::utils::tron_address::normalize_tron_address;

pub const ENTITY_LABEL_SUBJECT: &str = "ENTITY_LABEL";
pub const CLUSTER_CLAIM_SUBJECT: &str = "CLUSTER_CLAIM";
const APPROVED: &str = "APPROVED";
const REJECTED: &str = "REJECTED";
const PENDING: &str = "PENDING";

#[derive(Debug, Clone, Deserialize)]
pub struct IntelligenceSourceInput {
    pub source_id: String,
    pub source_name: String,
    pub source_type: String,
    pub trust_tier: String,
    #[serde(default)]
    pub reference_url: String,
    #[serde(default)]
    pub license: String,
    #[serde(default = "default_active")]
    pub is_active: bool,
    pub created_by: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntityLabelSubmission {
    pub address: String,
    #[serde(default)]
    pub label_id: String,
    #[serde(default)]
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    #[serde(default = "default_unknown_role")]
    pub address_role: String,
    pub confidence: f32,
    #[serde(default)]
    pub risk_percent: u8,
    pub source: String,
    #[serde(default)]
    pub source_record_id: String,
    #[serde(default)]
    pub supersedes_label_id: String,
    pub submitted_by: String,
    #[serde(default)]
    pub case_id: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IntelligenceReviewInput {
    pub subject_type: String,
    pub subject_id: String,
    pub decision: String,
    pub reviewer: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletIntelligence {
    pub address: String,
    pub active_entity: Option<ActiveEntityAttribution>,
    pub active_exchange: Option<ActiveExchangeAttribution>,
    pub active_clusters: Vec<ActiveClusterMembership>,
    pub entity_claims: Vec<EntityClaimEvidence>,
    pub cluster_claims: Vec<ClusterClaimEvidence>,
    pub pending_review_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ActiveEntityAttribution {
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub confidence: f32,
    pub source_label_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ActiveExchangeAttribution {
    pub entity_id: String,
    pub exchange_name: String,
    pub address_role: String,
    pub confidence: f32,
    pub source_claim_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ActiveClusterMembership {
    pub cluster_id: String,
    pub cluster_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub source_claim_id: String,
    pub cluster_version: u32,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct EntityClaimEvidence {
    pub label_id: String,
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub risk_percent: u8,
    pub source_id: String,
    pub source_name: String,
    pub trust_tier: String,
    pub case_id: String,
    pub evidence_refs: Vec<String>,
    pub review_status: String,
    pub reviewer: String,
    pub review_reason: String,
    pub supersedes_label_id: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ClusterClaimEvidence {
    pub claim_id: String,
    pub cluster_id: String,
    pub cluster_type: String,
    pub address_role: String,
    pub claim_method: String,
    pub confidence: f32,
    pub source_id: String,
    pub source_name: String,
    pub trust_tier: String,
    pub evidence_tx_hashes: Vec<String>,
    pub evidence_addresses: Vec<String>,
    pub evidence_json: String,
    pub review_status: String,
    pub reviewer: String,
    pub review_reason: String,
    pub supersedes_claim_id: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct CurrentProjectionSource {
    source: String,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ClusterProjectionAnchor {
    entity_id: String,
    exchange_name: String,
    confidence: f32,
}

pub async fn register_intelligence_source(
    clickhouse: &Client,
    input: IntelligenceSourceInput,
) -> Result<IntelligenceSourceRow> {
    validate_non_empty("source_id", &input.source_id)?;
    validate_identifier("source_id", &input.source_id)?;
    validate_non_empty("source_name", &input.source_name)?;
    validate_non_empty("created_by", &input.created_by)?;
    let source_type = normalize_enum(
        "source_type",
        &input.source_type,
        &[
            "ANALYST",
            "LAW_ENFORCEMENT",
            "REGULATORY",
            "VENDOR",
            "PUBLIC_RESEARCH",
            "INTERNAL",
            "INTERNAL_BOOTSTRAP",
            "HEURISTIC",
        ],
    )?;
    let trust_tier = normalize_enum(
        "trust_tier",
        &input.trust_tier,
        &["VERIFIED", "HIGH", "MEDIUM", "UNVERIFIED"],
    )?;
    let row = IntelligenceSourceRow {
        chain: "tron".to_string(),
        source_id: input.source_id,
        source_name: input.source_name,
        source_type,
        trust_tier,
        reference_url: input.reference_url,
        license: input.license,
        is_active: u8::from(input.is_active),
        created_by: input.created_by,
        created_at_unix_ms: now_unix_ms(),
    };
    let mut insert = clickhouse
        .insert::<IntelligenceSourceRow>("intelligence_sources")
        .await?;
    insert.write(&row).await?;
    insert.end().await?;

    Ok(row)
}

pub async fn submit_entity_label(
    clickhouse: &Client,
    input: EntityLabelSubmission,
) -> Result<EntityLabelClaimRow> {
    let address = normalize_tron_address(&input.address)
        .ok_or_else(|| anyhow!("invalid TRON address: {}", input.address))?;
    validate_non_empty("entity_name", &input.entity_name)?;
    validate_non_empty("entity_type", &input.entity_type)?;
    validate_non_empty("source", &input.source)?;
    validate_non_empty("source_record_id", &input.source_record_id)?;
    validate_non_empty("submitted_by", &input.submitted_by)?;
    if input.evidence_refs.is_empty() {
        return Err(anyhow!("evidence_refs must contain at least one reference"));
    }
    if !(0.0..=1.0).contains(&input.confidence) {
        return Err(anyhow!("confidence must be between 0 and 1"));
    }
    if input.risk_percent > 100 {
        return Err(anyhow!("risk_percent must be between 0 and 100"));
    }
    require_active_source(clickhouse, &input.source).await?;

    let entity_id = if input.entity_id.trim().is_empty() {
        format!(
            "tron:{}:{}",
            slug(&input.entity_type),
            slug(&input.entity_name)
        )
    } else {
        input.entity_id.trim().to_string()
    };
    let address_role = normalize_role(&input.address_role)?;
    let label_id = if input.label_id.trim().is_empty() {
        content_id(
            "tron_label",
            &[
                &address,
                &entity_id,
                &address_role,
                &input.source,
                &input.source_record_id,
            ],
        )
    } else {
        input.label_id.trim().to_string()
    };
    let row = EntityLabelClaimRow {
        label_id,
        chain: "tron".to_string(),
        address,
        entity_id,
        entity_name: input.entity_name.trim().to_string(),
        entity_type: input.entity_type.trim().to_ascii_lowercase(),
        address_role,
        confidence: input.confidence,
        risk_percent: input.risk_percent,
        source: input.source,
        source_record_id: input.source_record_id,
        supersedes_label_id: input.supersedes_label_id,
        submitted_by: input.submitted_by,
        case_id: input.case_id,
        evidence_refs: input.evidence_refs,
        review_status: PENDING.to_string(),
        created_at_unix_ms: now_unix_ms(),
    };
    let mut insert = clickhouse
        .insert::<EntityLabelClaimRow>("entity_labels")
        .await?;
    insert.write(&row).await?;
    insert.end().await?;

    Ok(row)
}

pub async fn submit_cluster_claim(
    clickhouse: &Client,
    mut row: AddressClusterClaimRow,
) -> Result<()> {
    row.address = normalize_tron_address(&row.address)
        .ok_or_else(|| anyhow!("invalid TRON address: {}", row.address))?;
    validate_non_empty("cluster_id", &row.cluster_id)?;
    validate_non_empty("claim_method", &row.claim_method)?;
    validate_non_empty("source", &row.source)?;
    validate_non_empty("created_by", &row.created_by)?;
    if !(0.0..=1.0).contains(&row.confidence) {
        return Err(anyhow!("confidence must be between 0 and 1"));
    }
    require_active_source(clickhouse, &row.source).await?;
    row.address_role = normalize_role(&row.address_role)?;
    row.review_status = PENDING.to_string();

    let mut insert = clickhouse
        .insert::<AddressClusterClaimRow>("address_cluster_claims")
        .await?;
    insert.write(&row).await?;
    insert.end().await?;
    Ok(())
}

pub async fn submit_cluster_claims(
    clickhouse: &Client,
    mut rows: Vec<AddressClusterClaimRow>,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut sources = rows
        .iter()
        .map(|row| row.source.clone())
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    for source in sources {
        require_active_source(clickhouse, &source).await?;
    }

    for row in &mut rows {
        row.address = normalize_tron_address(&row.address)
            .ok_or_else(|| anyhow!("invalid TRON address: {}", row.address))?;
        validate_non_empty("cluster_id", &row.cluster_id)?;
        validate_non_empty("claim_method", &row.claim_method)?;
        validate_non_empty("created_by", &row.created_by)?;
        if !(0.0..=1.0).contains(&row.confidence) {
            return Err(anyhow!("confidence must be between 0 and 1"));
        }
        row.address_role = normalize_role(&row.address_role)?;
        row.review_status = PENDING.to_string();
    }

    let row_count = rows.len();
    let mut insert = clickhouse
        .insert::<AddressClusterClaimRow>("address_cluster_claims")
        .await?;
    for row in &rows {
        insert.write(row).await?;
    }
    insert.end().await?;
    Ok(row_count)
}

pub async fn review_intelligence_subject(
    clickhouse: Arc<Client>,
    input: IntelligenceReviewInput,
) -> Result<IntelligenceReviewRow> {
    let subject_type = normalize_enum(
        "subject_type",
        &input.subject_type,
        &[ENTITY_LABEL_SUBJECT, CLUSTER_CLAIM_SUBJECT],
    )?;
    let decision = normalize_enum("decision", &input.decision, &[APPROVED, REJECTED])?;
    validate_non_empty("subject_id", &input.subject_id)?;
    validate_non_empty("reviewer", &input.reviewer)?;

    let row = IntelligenceReviewRow {
        review_id: format!("tron_review_{}", nanoid!(20)),
        chain: "tron".to_string(),
        subject_type: subject_type.clone(),
        subject_id: input.subject_id,
        decision: decision.clone(),
        reviewer: input.reviewer,
        reason: input.reason,
        evidence_refs: input.evidence_refs,
        created_at_unix_ms: now_unix_ms(),
    };

    match subject_type.as_str() {
        ENTITY_LABEL_SUBJECT => {
            let claim = load_entity_claim(&clickhouse, &row.subject_id).await?;
            insert_review(&clickhouse, &row).await?;
            project_entity_review(&clickhouse, &claim, &row).await?;
        }
        CLUSTER_CLAIM_SUBJECT => {
            let claim = load_cluster_claim(&clickhouse, &row.subject_id).await?;
            insert_review(&clickhouse, &row).await?;
            project_cluster_review(&clickhouse, &claim, &row).await?;
        }
        _ => unreachable!(),
    }

    Ok(row)
}

pub async fn load_wallet_intelligence(
    clickhouse: Arc<Client>,
    address: &str,
) -> Result<WalletIntelligence> {
    let active_entity = clickhouse
        .query(
            r#"
            SELECT
                entity_id,
                entity_name,
                entity_type,
                confidence,
                source AS source_label_id
            FROM address_entity FINAL
            WHERE address = ? AND is_active = 1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(address)
        .fetch_optional::<ActiveEntityAttribution>()
        .await?;
    let active_exchange = clickhouse
        .query(
            r#"
            SELECT
                entity_id,
                exchange_name,
                address_role,
                confidence,
                detection_source AS source_claim_id
            FROM exchange_addresses FINAL
            WHERE address = ? AND is_active = 1
            LIMIT 1
            "#,
        )
        .bind(address)
        .fetch_optional::<ActiveExchangeAttribution>()
        .await?;
    let active_clusters = clickhouse
        .query(
            r#"
            SELECT
                membership.cluster_id,
                membership.cluster_type,
                membership.address_role,
                membership.confidence,
                membership.source_claim_id,
                membership.cluster_version,
                ifNull(version.display_name, membership.cluster_id) AS display_name
            FROM address_cluster_memberships AS membership FINAL
            LEFT JOIN
            (
                SELECT
                    cluster_id,
                    argMax(display_name, version) AS display_name
                FROM cluster_versions FINAL
                WHERE chain = 'tron'
                GROUP BY cluster_id
            ) AS version ON version.cluster_id = membership.cluster_id
            WHERE membership.chain = 'tron'
              AND membership.address = ?
              AND membership.is_active = 1
            ORDER BY membership.confidence DESC, membership.cluster_id
            "#,
        )
        .bind(address)
        .fetch_all::<ActiveClusterMembership>()
        .await?;
    let entity_claims = load_entity_claim_evidence(&clickhouse, address).await?;
    let cluster_claims = load_cluster_claim_evidence(&clickhouse, address).await?;
    let pending_review_count = entity_claims
        .iter()
        .filter(|claim| claim.review_status == PENDING)
        .count()
        + cluster_claims
            .iter()
            .filter(|claim| claim.review_status == PENDING)
            .count();

    Ok(WalletIntelligence {
        address: address.to_string(),
        active_entity,
        active_exchange,
        active_clusters,
        entity_claims,
        cluster_claims,
        pending_review_count,
    })
}

async fn require_active_source(clickhouse: &Client, source_id: &str) -> Result<()> {
    let exists = clickhouse
        .query(
            r#"
            SELECT count()
            FROM intelligence_sources FINAL
            WHERE chain = 'tron'
              AND source_id = ?
              AND is_active = 1
            "#,
        )
        .bind(source_id)
        .fetch_one::<u64>()
        .await?;
    if exists == 0 {
        return Err(anyhow!(
            "intelligence source {source_id:?} is not registered and active"
        ));
    }
    Ok(())
}

async fn insert_review(clickhouse: &Client, row: &IntelligenceReviewRow) -> Result<()> {
    let mut insert = clickhouse
        .insert::<IntelligenceReviewRow>("intelligence_reviews")
        .await?;
    insert.write(row).await?;
    insert.end().await?;
    Ok(())
}

async fn load_entity_claim(clickhouse: &Client, label_id: &str) -> Result<EntityLabelClaimRow> {
    clickhouse
        .query(
            r#"
            SELECT
                label_id,
                chain,
                address,
                entity_id,
                entity_name,
                entity_type,
                address_role,
                confidence,
                risk_percent,
                source,
                source_record_id,
                supersedes_label_id,
                submitted_by,
                case_id,
                evidence_refs,
                review_status,
                created_at_unix_ms
            FROM entity_labels FINAL
            WHERE chain = 'tron' AND label_id = ?
            LIMIT 1
            "#,
        )
        .bind(label_id)
        .fetch_optional::<EntityLabelClaimRow>()
        .await?
        .ok_or_else(|| anyhow!("entity label {label_id:?} does not exist"))
}

async fn load_cluster_claim(clickhouse: &Client, claim_id: &str) -> Result<AddressClusterClaimRow> {
    clickhouse
        .query(
            r#"
            SELECT
                claim_id,
                chain,
                address,
                cluster_id,
                cluster_type,
                address_role,
                claim_method,
                confidence,
                source,
                source_record_id,
                evidence_tx_hashes,
                evidence_addresses,
                evidence_json,
                supersedes_claim_id,
                review_status,
                created_by,
                created_at_unix_ms
            FROM address_cluster_claims FINAL
            WHERE chain = 'tron' AND claim_id = ?
            LIMIT 1
            "#,
        )
        .bind(claim_id)
        .fetch_optional::<AddressClusterClaimRow>()
        .await?
        .ok_or_else(|| anyhow!("cluster claim {claim_id:?} does not exist"))
}

async fn project_entity_review(
    clickhouse: &Client,
    claim: &EntityLabelClaimRow,
    review: &IntelligenceReviewRow,
) -> Result<()> {
    let approved = review.decision == APPROVED;
    if approved
        || projection_matches(
            clickhouse,
            "address_entity",
            &claim.address,
            &claim.label_id,
        )
        .await?
    {
        let mut insert = clickhouse
            .insert::<AddressEntityRow>("address_entity")
            .await?;
        insert
            .write(&AddressEntityRow {
                address: claim.address.clone(),
                entity_id: claim.entity_id.clone(),
                entity_name: claim.entity_name.clone(),
                entity_type: claim.entity_type.clone(),
                confidence: claim.confidence,
                source: claim.label_id.clone(),
                is_active: u8::from(approved),
            })
            .await?;
        insert.end().await?;
    }

    if is_exchange_entity_type(&claim.entity_type)
        && (approved
            || projection_matches(
                clickhouse,
                "exchange_addresses",
                &claim.address,
                &format!("entity_label:{}", claim.label_id),
            )
            .await?)
    {
        // اینجا در clickhouse ذخیره می شود
        // address:
        //     TXYZ...
        //     entity_id:
        //         tron:centralized_exchange:binance
        // exchange_name:
        //     Binance
        // address_role:
        //     HOT
        // confidence:
        // 0.99
        // detection_source:
        //     entity_label:abc...
        //     is_active:
        // 1
        insert_exchange_projection(
            clickhouse,
            ExchangeAddressRow {
                address: claim.address.clone(),
                entity_id: claim.entity_id.clone(),
                exchange_name: claim.entity_name.clone(),
                address_role: claim.address_role.clone(),
                confidence: claim.confidence,
                detection_source: format!("entity_label:{}", claim.label_id),
                first_seen_block: 0,
                last_seen_block: 0,
                is_active: u8::from(approved),
            },
        )
        .await?;
    }

    if claim.risk_percent > 0
        && (approved || current_exposure_seed_matches(clickhouse, claim).await?)
    {
        let mut insert = clickhouse
            .insert::<ExposureSeedRow>("exposure_seeds")
            .await?;
        insert
            .write(&ExposureSeedRow {
                address: claim.address.clone(),
                entity_name: claim.entity_name.clone(),
                entity_type: claim.entity_type.clone(),
                risk_level: claim.risk_percent,
                source: claim.source.clone(),
                source_label_id: claim.label_id.clone(),
                is_active: u8::from(approved),
            })
            .await?;
        insert.end().await?;
    }

    Ok(())
}

async fn project_cluster_review(
    clickhouse: &Client,
    claim: &AddressClusterClaimRow,
    review: &IntelligenceReviewRow,
) -> Result<()> {
    let next_version = clickhouse
        .query(
            r#"
            SELECT toUInt32(ifNull(max(version), 0) + 1)
            FROM cluster_versions
            WHERE chain = 'tron' AND cluster_id = ?
            "#,
        )
        .bind(&claim.cluster_id)
        .fetch_one::<u32>()
        .await?;
    let approved = review.decision == APPROVED;
    let other_active_members = clickhouse
        .query(
            r#"
            SELECT count()
            FROM address_cluster_memberships FINAL
            WHERE chain = 'tron'
              AND cluster_id = ?
              AND address != ?
              AND is_active = 1
            "#,
        )
        .bind(&claim.cluster_id)
        .bind(&claim.address)
        .fetch_one::<u64>()
        .await?;

    let membership = AddressClusterMembershipRow {
        chain: "tron".to_string(),
        address: claim.address.clone(),
        cluster_id: claim.cluster_id.clone(),
        cluster_type: claim.cluster_type.clone(),
        address_role: claim.address_role.clone(),
        confidence: claim.confidence,
        source_claim_id: claim.claim_id.clone(),
        review_id: review.review_id.clone(),
        cluster_version: next_version,
        is_active: u8::from(approved),
        created_at_unix_ms: review.created_at_unix_ms,
    };
    let mut membership_insert = clickhouse
        .insert::<AddressClusterMembershipRow>("address_cluster_memberships")
        .await?;
    membership_insert.write(&membership).await?;
    membership_insert.end().await?;

    let display_name = cluster_display_name(clickhouse, claim).await?;
    let version = ClusterVersionRow {
        chain: "tron".to_string(),
        cluster_id: claim.cluster_id.clone(),
        version: next_version,
        cluster_type: claim.cluster_type.clone(),
        display_name,
        change_type: if approved {
            "MEMBER_APPROVED".to_string()
        } else {
            "MEMBER_REJECTED".to_string()
        },
        change_reason: review.reason.clone(),
        source_claim_ids: vec![claim.claim_id.clone()],
        active_member_count: other_active_members + u64::from(approved),
        created_by: review.reviewer.clone(),
        created_at_unix_ms: review.created_at_unix_ms,
    };
    let mut version_insert = clickhouse
        .insert::<ClusterVersionRow>("cluster_versions")
        .await?;
    version_insert.write(&version).await?;
    version_insert.end().await?;

    project_cluster_exchange_address(clickhouse, claim, approved).await
}

async fn project_cluster_exchange_address(
    clickhouse: &Client,
    claim: &AddressClusterClaimRow,
    approved: bool,
) -> Result<()> {
    let detection_source = format!("cluster_claim:{}", claim.claim_id);
    if !approved
        && !projection_matches(
            clickhouse,
            "exchange_addresses",
            &claim.address,
            &detection_source,
        )
        .await?
    {
        return Ok(());
    }

    let anchor = load_cluster_projection_anchor(clickhouse, &claim.evidence_addresses).await?;
    let Some(anchor) = anchor else {
        return Ok(());
    };
    insert_exchange_projection(
        clickhouse,
        ExchangeAddressRow {
            address: claim.address.clone(),
            entity_id: anchor.entity_id,
            exchange_name: anchor.exchange_name,
            address_role: claim.address_role.clone(),
            confidence: claim.confidence.min(anchor.confidence),
            detection_source,
            first_seen_block: 0,
            last_seen_block: 0,
            is_active: u8::from(approved),
        },
    )
    .await
}

async fn insert_exchange_projection(clickhouse: &Client, row: ExchangeAddressRow) -> Result<()> {
    let mut insert = clickhouse
        .insert::<ExchangeAddressRow>("exchange_addresses")
        .await?;
    insert.write(&row).await?;
    insert.end().await?;
    Ok(())
}

async fn projection_matches(
    clickhouse: &Client,
    table: &str,
    address: &str,
    expected_source: &str,
) -> Result<bool> {
    let source_column = match table {
        "address_entity" => "source",
        "exchange_addresses" => "detection_source",
        _ => return Err(anyhow!("unsupported projection table {table}")),
    };
    let sql =
        format!("SELECT {source_column} AS source FROM {table} FINAL WHERE address = ? LIMIT 1");
    let current = clickhouse
        .query(&sql)
        .bind(address)
        .fetch_optional::<CurrentProjectionSource>()
        .await?;
    Ok(current.is_some_and(|row| row.source == expected_source))
}

async fn current_exposure_seed_matches(
    clickhouse: &Client,
    claim: &EntityLabelClaimRow,
) -> Result<bool> {
    let source_label_id = clickhouse
        .query(
            r#"
            SELECT source_label_id
            FROM exposure_seeds FINAL
            WHERE address = ?
            LIMIT 1
            "#,
        )
        .bind(&claim.address)
        .fetch_optional::<String>()
        .await?;
    Ok(source_label_id.as_deref() == Some(claim.label_id.as_str()))
}

async fn load_cluster_projection_anchor(
    clickhouse: &Client,
    evidence_addresses: &[String],
) -> Result<Option<ClusterProjectionAnchor>> {
    if evidence_addresses.is_empty() {
        return Ok(None);
    }
    clickhouse
        .query(
            r#"
            SELECT entity_id, exchange_name, confidence
            FROM exchange_addresses FINAL
            WHERE address IN ? AND is_active = 1
            ORDER BY confidence DESC
            LIMIT 1
            "#,
        )
        .bind(evidence_addresses)
        .fetch_optional::<ClusterProjectionAnchor>()
        .await
        .map_err(Into::into)
}

async fn cluster_display_name(
    clickhouse: &Client,
    claim: &AddressClusterClaimRow,
) -> Result<String> {
    Ok(
        load_cluster_projection_anchor(clickhouse, &claim.evidence_addresses)
            .await?
            .map(|anchor| format!("{} {} cluster", anchor.exchange_name, claim.address_role))
            .unwrap_or_else(|| claim.cluster_id.clone()),
    )
}

async fn load_entity_claim_evidence(
    clickhouse: &Client,
    address: &str,
) -> Result<Vec<EntityClaimEvidence>> {
    clickhouse
        .query(
            r#"
            SELECT
                label.label_id AS label_id,
                label.entity_id AS entity_id,
                label.entity_name AS entity_name,
                label.entity_type AS entity_type,
                label.address_role AS address_role,
                label.confidence AS confidence,
                label.risk_percent AS risk_percent,
                label.source AS source_id,
                if(source.source_name = '', label.source, source.source_name) AS source_name,
                source.trust_tier AS trust_tier,
                label.case_id AS case_id,
                label.evidence_refs AS evidence_refs,
                if(review.decision = '', label.review_status, review.decision) AS review_status,
                review.reviewer AS reviewer,
                review.reason AS review_reason,
                label.supersedes_label_id AS supersedes_label_id,
                label.created_at_unix_ms AS created_at_unix_ms
            FROM entity_labels AS label FINAL
            LEFT JOIN
            (
                SELECT
                    subject_id,
                    argMax(decision, created_at_unix_ms) AS decision,
                    argMax(reviewer, created_at_unix_ms) AS reviewer,
                    argMax(reason, created_at_unix_ms) AS reason
                FROM intelligence_reviews FINAL
                WHERE chain = 'tron' AND subject_type = 'ENTITY_LABEL'
                GROUP BY subject_id
            ) AS review ON review.subject_id = label.label_id
            LEFT JOIN intelligence_sources AS source FINAL
                ON source.chain = label.chain AND source.source_id = label.source
            WHERE label.chain = 'tron' AND label.address = ?
            ORDER BY label.created_at_unix_ms DESC
            LIMIT 50
            "#,
        )
        .bind(address)
        .fetch_all::<EntityClaimEvidence>()
        .await
        .context("failed to load governed entity label evidence")
}

async fn load_cluster_claim_evidence(
    clickhouse: &Client,
    address: &str,
) -> Result<Vec<ClusterClaimEvidence>> {
    clickhouse
        .query(
            r#"
            SELECT
                claim.claim_id AS claim_id,
                claim.cluster_id AS cluster_id,
                claim.cluster_type AS cluster_type,
                claim.address_role AS address_role,
                claim.claim_method AS claim_method,
                claim.confidence AS confidence,
                claim.source AS source_id,
                if(source.source_name = '', claim.source, source.source_name) AS source_name,
                source.trust_tier AS trust_tier,
                claim.evidence_tx_hashes AS evidence_tx_hashes,
                claim.evidence_addresses AS evidence_addresses,
                claim.evidence_json AS evidence_json,
                if(review.decision = '', claim.review_status, review.decision) AS review_status,
                review.reviewer AS reviewer,
                review.reason AS review_reason,
                claim.supersedes_claim_id AS supersedes_claim_id,
                claim.created_at_unix_ms AS created_at_unix_ms
            FROM address_cluster_claims AS claim FINAL
            LEFT JOIN
            (
                SELECT
                    subject_id,
                    argMax(decision, created_at_unix_ms) AS decision,
                    argMax(reviewer, created_at_unix_ms) AS reviewer,
                    argMax(reason, created_at_unix_ms) AS reason
                FROM intelligence_reviews FINAL
                WHERE chain = 'tron' AND subject_type = 'CLUSTER_CLAIM'
                GROUP BY subject_id
            ) AS review ON review.subject_id = claim.claim_id
            LEFT JOIN intelligence_sources AS source FINAL
                ON source.chain = claim.chain AND source.source_id = claim.source
            WHERE claim.chain = 'tron' AND claim.address = ?
            ORDER BY claim.created_at_unix_ms DESC
            LIMIT 50
            "#,
        )
        .bind(address)
        .fetch_all::<ClusterClaimEvidence>()
        .await
        .context("failed to load address cluster claim evidence")
}

fn normalize_role(value: &str) -> Result<String> {
    normalize_enum(
        "address_role",
        value,
        &[
            "UNKNOWN", "HOT", "DEPOSIT", "SWEEP", "TREASURY", "WITHDRAW", "INTERNAL", "SERVICE",
            "CUSTOMER", "OPERATOR", "CONTRACT",
        ],
    )
}

fn normalize_enum(field: &str, value: &str, allowed: &[&str]) -> Result<String> {
    let normalized = value.trim().to_ascii_uppercase();
    if allowed.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(anyhow!(
            "{field} must be one of {}; got {:?}",
            allowed.join(", "),
            value
        ))
    }
}

fn validate_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(anyhow!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_identifier(field: &str, value: &str) -> Result<()> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "{field} may contain only ASCII letters, numbers, underscore, dash, colon, and dot"
        ))
    }
}

fn content_id(prefix: &str, parts: &[&str]) -> String {
    let identity = parts.join("|");
    let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
    format!("{prefix}_{}", &digest[..24])
}

fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut previous_separator = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
            previous_separator = false;
        } else if !previous_separator && !result.is_empty() {
            result.push('_');
            previous_separator = true;
        }
    }
    result.trim_matches('_').to_string()
}

fn is_exchange_entity_type(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "exchange" | "centralized_exchange" | "dex" | "custodial_exchange"
    )
}

fn now_unix_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

const fn default_active() -> bool {
    true
}

fn default_unknown_role() -> String {
    "UNKNOWN".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_ids_are_stable_and_change_with_source_record() {
        let first = content_id("label", &["address", "entity", "source", "1"]);
        let replay = content_id("label", &["address", "entity", "source", "1"]);
        let changed = content_id("label", &["address", "entity", "source", "2"]);

        assert_eq!(first, replay);
        assert_ne!(first, changed);
    }

    #[test]
    fn source_identifiers_reject_whitespace_and_shell_punctuation() {
        assert!(validate_identifier("source", "law_enforcement:case-1").is_ok());
        assert!(validate_identifier("source", "bad source").is_err());
        assert!(validate_identifier("source", "bad;source").is_err());
    }

    #[test]
    fn roles_are_normalized_and_bounded() {
        assert_eq!(normalize_role(" deposit ").unwrap(), "DEPOSIT");
        assert!(normalize_role("probably exchange").is_err());
    }

    #[test]
    fn entity_slug_is_stable() {
        assert_eq!(slug("Centralized Exchange"), "centralized_exchange");
        assert_eq!(slug("Binance.com"), "binance_com");
    }
}
