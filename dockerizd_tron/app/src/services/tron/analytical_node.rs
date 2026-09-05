use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use chrono::Utc;
use clickhouse::Client;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::services::tron::wallet_investigation::{
    WalletInvestigation, WalletInvestigationOptions, build_wallet_investigation,
};

const CHAIN: &str = "tron";
const SUBJECT_TYPE_WALLET: &str = "wallet";
const ANALYSIS_VERSION: &str = "tron_wallet_analysis_v1";
const SNAPSHOT_TABLE: &str = "wallet_analysis_snapshots";
const EVIDENCE_TABLE: &str = "wallet_analysis_evidence";
const SUBJECT_TABLE: &str = "analysis_subjects";

#[derive(Debug, Clone, Serialize)]
pub struct WalletAnalysisSnapshotResponse {
    pub snapshot_id: String,
    pub chain: String,
    pub address: String,
    pub entity_id: Option<String>,
    pub analysis_version: String,
    pub analysis_status: String,
    pub risk_available: bool,
    pub risk_level: String,
    pub risk_probability: f32,
    pub risk_percent: u8,
    pub confidence: f32,
    pub wallet_type: String,
    pub fingerprint_label: String,
    pub data_cutoff_block: u64,
    pub data_cutoff_unix_ms: u64,
    pub analysis_input_version: String,
    pub created_at_unix_ms: u64,
    pub source_tables: Vec<String>,
    pub warnings: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub evidence: Vec<WalletAnalysisEvidence>,
    pub snapshot: Value,
    pub persistence: WalletAnalysisPersistence,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletAnalysisPersistence {
    pub source: String,
    pub snapshot_table: String,
    pub evidence_table: String,
    pub subject_table: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct WalletAnalysisEvidence {
    pub evidence_id: String,
    pub snapshot_id: String,
    pub chain: String,
    pub address: String,
    pub evidence_type: String,
    pub evidence_key: String,
    pub evidence_value: String,
    pub severity: String,
    pub related_tx_hash: String,
    pub related_address: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Serialize, clickhouse::Row)]
struct AnalysisSubjectInsertRow {
    chain: String,
    subject_type: String,
    subject_id: String,
    address: String,
    entity_id: String,
    latest_snapshot_id: String,
    latest_status: String,
    latest_risk_available: u8,
    latest_risk_level: String,
    latest_risk_probability: f32,
    latest_confidence: f32,
    latest_data_cutoff_block: u64,
    latest_data_cutoff_unix_ms: u64,
    latest_input_version: String,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Serialize, clickhouse::Row)]
struct WalletAnalysisSnapshotInsertRow {
    snapshot_id: String,
    chain: String,
    address: String,
    entity_id: String,
    analysis_version: String,
    analysis_status: String,
    risk_available: u8,
    risk_level: String,
    risk_probability: f32,
    risk_percent: u8,
    confidence: f32,
    wallet_type: String,
    fingerprint_label: String,
    graph_depth: u8,
    graph_node_count: u32,
    graph_edge_count: u32,
    exchange_interaction_count: u32,
    holdings_asset_count: u64,
    holdings_metadata_gap_count: u32,
    observed_transfers: u64,
    incoming_transfers: u64,
    outgoing_transfers: u64,
    exposure_score: f32,
    exposure_source_count: u32,
    exposure_path_count: u64,
    exposure_min_hop_distance: u8,
    data_cutoff_block: u64,
    data_cutoff_unix_ms: u64,
    analysis_input_version: String,
    source_tables: Vec<String>,
    model_id: String,
    model_version: String,
    feature_schema_version: String,
    snapshot_json: String,
    warnings: Vec<String>,
    evidence_refs: Vec<String>,
    created_at_unix_ms: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct WalletAnalysisSnapshotReadRow {
    snapshot_id: String,
    chain: String,
    address: String,
    entity_id: String,
    analysis_version: String,
    analysis_status: String,
    risk_available: u8,
    risk_level: String,
    risk_probability: f32,
    risk_percent: u8,
    confidence: f32,
    wallet_type: String,
    fingerprint_label: String,
    data_cutoff_block: u64,
    data_cutoff_unix_ms: u64,
    analysis_input_version: String,
    source_tables: Vec<String>,
    snapshot_json: String,
    warnings: Vec<String>,
    evidence_refs: Vec<String>,
    created_at_unix_ms: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct IndexedCutoffRow {
    block_number: u64,
    block_timestamp: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct AnalysisInputVersionRow {
    analysis_input_version: String,
}

pub async fn get_or_create_wallet_analysis_snapshot(
    clickhouse: Arc<Client>,
    address: &str,
    options: WalletInvestigationOptions,
    refresh: bool,
) -> anyhow::Result<WalletAnalysisSnapshotResponse> {
    if !refresh {
        let indexed_cutoff = load_indexed_cutoff(&clickhouse).await?;
        let input_version = load_analysis_input_version(&clickhouse, address).await?;

        if let Some(snapshot) =
            load_latest_wallet_analysis_snapshot(clickhouse.clone(), address).await?
            && snapshot.data_cutoff_block >= indexed_cutoff.0
            && snapshot.analysis_input_version == input_version
        {
            return Ok(snapshot);
        }
    }

    create_wallet_analysis_snapshot(clickhouse, address, options).await
}

async fn create_wallet_analysis_snapshot(
    clickhouse: Arc<Client>,
    address: &str,
    options: WalletInvestigationOptions,
) -> anyhow::Result<WalletAnalysisSnapshotResponse> {
    let (data_cutoff_block, data_cutoff_unix_ms) = load_indexed_cutoff(&clickhouse).await?;
    let analysis_input_version = load_analysis_input_version(&clickhouse, address).await?;
    let investigation = build_wallet_investigation(clickhouse.clone(), address, options).await?;
    let created_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let snapshot_id = format!(
        "tron_wallet_analysis_{}_{}",
        created_at_unix_ms,
        nanoid!(10)
    );
    let entity_id = investigation
        .fingerprint
        .identity
        .entity_id
        .clone()
        .unwrap_or_default();
    let risk_available = investigation.ai_risk.risk_score.is_some();
    let risk_probability = investigation.ai_risk.risk_score.unwrap_or_default();
    let risk_percent = investigation
        .ai_risk
        .risk_percent
        .unwrap_or_else(|| (risk_probability * 100.0).round().clamp(0.0, 100.0) as u8);
    let confidence = investigation
        .ai_risk
        .confidence
        .unwrap_or(investigation.fingerprint.confidence);
    let evidence_refs = collect_evidence_refs(&investigation);
    let source_tables = vec![
        "transactions_canonical".to_string(),
        "address_relationships_canonical".to_string(),
        "semantic_aml_events".to_string(),
        "wallet_asset_balances".to_string(),
        "address_exposure".to_string(),
        "exposure_runs".to_string(),
        "address_entity".to_string(),
        "exchange_flows_canonical".to_string(),
        "wallet_ml_model_deployments".to_string(),
    ];
    let evidence = build_evidence_rows(
        &snapshot_id,
        address,
        &investigation,
        &evidence_refs,
        created_at_unix_ms,
    );
    let snapshot = json!({
        "snapshot_id": snapshot_id,
        "chain": CHAIN,
        "subject": {
            "subject_type": SUBJECT_TYPE_WALLET,
            "address": address,
            "entity_id": empty_to_json(&entity_id)
        },
        "analysis": {
            "version": ANALYSIS_VERSION,
            "status": investigation.ai_risk.status,
            "created_at_unix_ms": created_at_unix_ms,
            "data_cutoff_block": data_cutoff_block,
            "data_cutoff_unix_ms": data_cutoff_unix_ms,
            "analysis_input_version": analysis_input_version,
            "source_tables": source_tables,
        },
        "risk": {
            "level": investigation.ai_risk.risk_level,
            "probability": risk_probability,
            "percent": risk_percent,
            "confidence": confidence,
            "model_id": investigation.ai_risk.model_id,
            "model_version": investigation.ai_risk.model_version,
            "feature_schema_version": investigation.ai_risk.feature_schema_version,
        },
        "summary": {
            "wallet_type": investigation.fingerprint.wallet_type,
            "fingerprint_label": investigation.fingerprint.fingerprint_label,
            "observed_transfers": investigation.fingerprint.flows.total_transfers,
            "incoming_transfers": investigation.fingerprint.flows.incoming_transfers,
            "outgoing_transfers": investigation.fingerprint.flows.outgoing_transfers,
            "graph_nodes": investigation.graph.nodes.len(),
            "graph_edges": investigation.graph.edges.len(),
            "holdings_asset_count": investigation.holdings.total_asset_count,
            "exposure_score": investigation.ai_risk.feature_snapshot.features.exposure_score,
            "exposure_source_count": investigation.ai_risk.feature_snapshot.features.exposure_source_count,
            "exposure_path_count": investigation.ai_risk.feature_snapshot.features.exposure_path_count,
        },
        "data_quality": &investigation.data_quality,
        "evidence": &evidence,
        "investigation": &investigation,
    });
    let snapshot_json = serde_json::to_string(&snapshot)?;

    let snapshot_row = WalletAnalysisSnapshotInsertRow {
        snapshot_id: snapshot_id.clone(),
        chain: CHAIN.to_string(),
        address: address.to_string(),
        entity_id: entity_id.clone(),
        analysis_version: ANALYSIS_VERSION.to_string(),
        analysis_status: investigation.ai_risk.status.clone(),
        risk_available: risk_available as u8,
        risk_level: investigation.ai_risk.risk_level.clone(),
        risk_probability,
        risk_percent,
        confidence,
        wallet_type: investigation.fingerprint.wallet_type.clone(),
        fingerprint_label: investigation.fingerprint.fingerprint_label.clone(),
        graph_depth: investigation.graph.depth,
        graph_node_count: investigation.graph.nodes.len() as u32,
        graph_edge_count: investigation.graph.edges.len() as u32,
        exchange_interaction_count: investigation.graph.exchange_interactions.len() as u32,
        holdings_asset_count: investigation.holdings.total_asset_count,
        holdings_metadata_gap_count: investigation.holdings.metadata_gap_count as u32,
        observed_transfers: investigation.fingerprint.flows.total_transfers,
        incoming_transfers: investigation.fingerprint.flows.incoming_transfers,
        outgoing_transfers: investigation.fingerprint.flows.outgoing_transfers,
        exposure_score: investigation
            .ai_risk
            .feature_snapshot
            .features
            .exposure_score,
        exposure_source_count: investigation
            .ai_risk
            .feature_snapshot
            .features
            .exposure_source_count,
        exposure_path_count: investigation
            .ai_risk
            .feature_snapshot
            .features
            .exposure_path_count,
        exposure_min_hop_distance: exposure_min_hop_distance(&investigation),
        data_cutoff_block,
        data_cutoff_unix_ms,
        analysis_input_version: analysis_input_version.clone(),
        source_tables: source_tables.clone(),
        model_id: investigation.ai_risk.model_id.clone().unwrap_or_default(),
        model_version: investigation
            .ai_risk
            .model_version
            .clone()
            .unwrap_or_default(),
        feature_schema_version: investigation.ai_risk.feature_schema_version.clone(),
        snapshot_json,
        warnings: investigation.data_quality.warnings.clone(),
        evidence_refs: evidence_refs.clone(),
        created_at_unix_ms,
    };

    persist_wallet_analysis_snapshot(clickhouse.clone(), &snapshot_row, &evidence).await?;
    persist_analysis_subject(
        clickhouse,
        &snapshot_row,
        SUBJECT_TYPE_WALLET,
        address,
        &entity_id,
        created_at_unix_ms,
    )
    .await?;

    Ok(WalletAnalysisSnapshotResponse {
        snapshot_id,
        chain: CHAIN.to_string(),
        address: address.to_string(),
        entity_id: non_empty(entity_id),
        analysis_version: ANALYSIS_VERSION.to_string(),
        analysis_status: investigation.ai_risk.status,
        risk_available,
        risk_level: investigation.ai_risk.risk_level,
        risk_probability,
        risk_percent,
        confidence,
        wallet_type: investigation.fingerprint.wallet_type,
        fingerprint_label: investigation.fingerprint.fingerprint_label,
        data_cutoff_block,
        data_cutoff_unix_ms,
        analysis_input_version,
        created_at_unix_ms,
        source_tables,
        warnings: investigation.data_quality.warnings,
        evidence_refs,
        evidence,
        snapshot,
        persistence: persistence("generated"),
    })
}

async fn persist_wallet_analysis_snapshot(
    clickhouse: Arc<Client>,
    snapshot: &WalletAnalysisSnapshotInsertRow,
    evidence: &[WalletAnalysisEvidence],
) -> anyhow::Result<()> {
    let mut snapshot_insert = clickhouse
        .insert::<WalletAnalysisSnapshotInsertRow>(SNAPSHOT_TABLE)
        .await
        .context("failed to open TRON wallet analysis snapshot insert")?;
    snapshot_insert.write(snapshot).await?;
    snapshot_insert.end().await?;

    if !evidence.is_empty() {
        let mut evidence_insert = clickhouse
            .insert::<WalletAnalysisEvidence>(EVIDENCE_TABLE)
            .await
            .context("failed to open TRON wallet analysis evidence insert")?;
        for item in evidence {
            evidence_insert.write(item).await?;
        }
        evidence_insert.end().await?;
    }

    Ok(())
}

async fn persist_analysis_subject(
    clickhouse: Arc<Client>,
    snapshot: &WalletAnalysisSnapshotInsertRow,
    subject_type: &str,
    subject_id: &str,
    entity_id: &str,
    now_unix_ms: u64,
) -> anyhow::Result<()> {
    let row = AnalysisSubjectInsertRow {
        chain: CHAIN.to_string(),
        subject_type: subject_type.to_string(),
        subject_id: subject_id.to_string(),
        address: snapshot.address.clone(),
        entity_id: entity_id.to_string(),
        latest_snapshot_id: snapshot.snapshot_id.clone(),
        latest_status: snapshot.analysis_status.clone(),
        latest_risk_available: snapshot.risk_available,
        latest_risk_level: snapshot.risk_level.clone(),
        latest_risk_probability: snapshot.risk_probability,
        latest_confidence: snapshot.confidence,
        latest_data_cutoff_block: snapshot.data_cutoff_block,
        latest_data_cutoff_unix_ms: snapshot.data_cutoff_unix_ms,
        latest_input_version: snapshot.analysis_input_version.clone(),
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
    };

    let mut insert = clickhouse
        .insert::<AnalysisSubjectInsertRow>(SUBJECT_TABLE)
        .await
        .context("failed to open TRON analysis subject insert")?;
    insert.write(&row).await?;
    insert.end().await?;

    Ok(())
}

async fn load_latest_wallet_analysis_snapshot(
    clickhouse: Arc<Client>,
    address: &str,
) -> anyhow::Result<Option<WalletAnalysisSnapshotResponse>> {
    let row = clickhouse
        .query(
            r#"
            SELECT
                snapshot_id,
                chain,
                address,
                entity_id,
                analysis_version,
                analysis_status,
                risk_available,
                risk_level,
                risk_probability,
                risk_percent,
                confidence,
                wallet_type,
                fingerprint_label,
                data_cutoff_block,
                data_cutoff_unix_ms,
                analysis_input_version,
                source_tables,
                snapshot_json,
                warnings,
                evidence_refs,
                created_at_unix_ms
            FROM wallet_analysis_snapshots
            WHERE chain = ?
              AND address = ?
            ORDER BY created_at_unix_ms DESC
            LIMIT 1
            "#,
        )
        .bind(CHAIN)
        .bind(address)
        .fetch_optional::<WalletAnalysisSnapshotReadRow>()
        .await
        .context("failed to load latest TRON wallet analysis snapshot")?;

    let Some(row) = row else {
        return Ok(None);
    };

    let evidence = load_wallet_analysis_evidence(clickhouse, &row.snapshot_id, address).await?;
    let snapshot = serde_json::from_str(&row.snapshot_json).unwrap_or_else(|error| {
        json!({
            "snapshot_id": row.snapshot_id,
            "parse_error": error.to_string(),
        })
    });

    Ok(Some(WalletAnalysisSnapshotResponse {
        snapshot_id: row.snapshot_id,
        chain: row.chain,
        address: row.address,
        entity_id: non_empty(row.entity_id),
        analysis_version: row.analysis_version,
        analysis_status: row.analysis_status,
        risk_available: row.risk_available == 1,
        risk_level: row.risk_level,
        risk_probability: row.risk_probability,
        risk_percent: row.risk_percent,
        confidence: row.confidence,
        wallet_type: row.wallet_type,
        fingerprint_label: row.fingerprint_label,
        data_cutoff_block: row.data_cutoff_block,
        data_cutoff_unix_ms: row.data_cutoff_unix_ms,
        analysis_input_version: row.analysis_input_version,
        created_at_unix_ms: row.created_at_unix_ms,
        source_tables: row.source_tables,
        warnings: row.warnings,
        evidence_refs: row.evidence_refs,
        evidence,
        snapshot,
        persistence: persistence("stored"),
    }))
}

async fn load_wallet_analysis_evidence(
    clickhouse: Arc<Client>,
    snapshot_id: &str,
    address: &str,
) -> anyhow::Result<Vec<WalletAnalysisEvidence>> {
    clickhouse
        .query(
            r#"
            SELECT
                evidence_id,
                snapshot_id,
                chain,
                address,
                evidence_type,
                evidence_key,
                evidence_value,
                severity,
                related_tx_hash,
                related_address,
                created_at_unix_ms
            FROM wallet_analysis_evidence
            WHERE chain = ?
              AND address = ?
              AND snapshot_id = ?
            ORDER BY evidence_type ASC, evidence_id ASC
            LIMIT 500
            "#,
        )
        .bind(CHAIN)
        .bind(address)
        .bind(snapshot_id)
        .fetch_all::<WalletAnalysisEvidence>()
        .await
        .context("failed to load TRON wallet analysis evidence")
}

fn build_evidence_rows(
    snapshot_id: &str,
    address: &str,
    investigation: &WalletInvestigation,
    evidence_refs: &[String],
    created_at_unix_ms: u64,
) -> Vec<WalletAnalysisEvidence> {
    let mut rows = Vec::new();

    for warning in &investigation.data_quality.warnings {
        rows.push(evidence_row(
            snapshot_id,
            address,
            created_at_unix_ms,
            EvidenceDraft {
                evidence_type: "data_quality",
                evidence_key: "warning",
                evidence_value: warning,
                severity: "warning",
                related_tx_hash: "",
                related_address: "",
            },
        ));
    }

    for item in &investigation.fingerprint.evidence {
        rows.push(evidence_row(
            snapshot_id,
            address,
            created_at_unix_ms,
            EvidenceDraft {
                evidence_type: "behavioral_fingerprint",
                evidence_key: "fingerprint_evidence",
                evidence_value: item,
                severity: "info",
                related_tx_hash: "",
                related_address: related_address(item).as_deref().unwrap_or_default(),
            },
        ));
    }

    for item in evidence_refs {
        rows.push(evidence_row(
            snapshot_id,
            address,
            created_at_unix_ms,
            EvidenceDraft {
                evidence_type: "risk_assessment",
                evidence_key: "risk_evidence_ref",
                evidence_value: item,
                severity: "info",
                related_tx_hash: related_tx_hash(item).as_deref().unwrap_or_default(),
                related_address: related_address(item).as_deref().unwrap_or_default(),
            },
        ));
    }

    for edge in investigation.graph.edges.iter().take(40) {
        let related_address = if edge.from == address {
            edge.to.as_str()
        } else {
            edge.from.as_str()
        };
        rows.push(evidence_row(
            snapshot_id,
            address,
            created_at_unix_ms,
            EvidenceDraft {
                evidence_type: "graph_edge",
                evidence_key: &edge.relationship_type,
                evidence_value: &format!(
                    "{} {} {} amount={} token={} block={}",
                    edge.from,
                    edge.relationship_type,
                    edge.to,
                    edge.amount,
                    edge.token_address,
                    edge.block_number
                ),
                severity: "info",
                related_tx_hash: &edge.tx_hash,
                related_address,
            },
        ));
    }

    dedupe_evidence(rows)
}

struct EvidenceDraft<'a> {
    evidence_type: &'a str,
    evidence_key: &'a str,
    evidence_value: &'a str,
    severity: &'a str,
    related_tx_hash: &'a str,
    related_address: &'a str,
}

fn evidence_row(
    snapshot_id: &str,
    address: &str,
    created_at_unix_ms: u64,
    draft: EvidenceDraft<'_>,
) -> WalletAnalysisEvidence {
    WalletAnalysisEvidence {
        evidence_id: format!("evidence_{}_{}", created_at_unix_ms, nanoid!(8)),
        snapshot_id: snapshot_id.to_string(),
        chain: CHAIN.to_string(),
        address: address.to_string(),
        evidence_type: draft.evidence_type.to_string(),
        evidence_key: draft.evidence_key.to_string(),
        evidence_value: draft.evidence_value.to_string(),
        severity: draft.severity.to_string(),
        related_tx_hash: draft.related_tx_hash.to_string(),
        related_address: draft.related_address.to_string(),
        created_at_unix_ms,
    }
}

fn dedupe_evidence(rows: Vec<WalletAnalysisEvidence>) -> Vec<WalletAnalysisEvidence> {
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|row| {
            seen.insert((
                row.evidence_type.clone(),
                row.evidence_key.clone(),
                row.evidence_value.clone(),
                row.related_tx_hash.clone(),
                row.related_address.clone(),
            ))
        })
        .collect()
}

fn collect_evidence_refs(investigation: &WalletInvestigation) -> Vec<String> {
    let mut refs = Vec::new();
    refs.extend(investigation.fingerprint.evidence.iter().cloned());
    refs.extend(investigation.ai_risk.evidence_refs.iter().cloned());
    refs.extend(investigation.data_quality.warnings.iter().cloned());
    refs.sort();
    refs.dedup();
    refs.truncate(250);
    refs
}

async fn load_indexed_cutoff(clickhouse: &Client) -> anyhow::Result<(u64, u64)> {
    let row = clickhouse
        .query(
            r#"
            SELECT
                max(block_number) AS block_number,
                argMax(block_timestamp, block_number) AS block_timestamp
            FROM ingested_blocks FINAL
            WHERE chain = 'tron'
              AND ingestion_status = 'COMPLETE'
            "#,
        )
        .fetch_one::<IndexedCutoffRow>()
        .await
        .context("failed to load TRON indexed data cutoff")?;

    Ok((row.block_number, row.block_timestamp))
}

async fn load_analysis_input_version(clickhouse: &Client, address: &str) -> anyhow::Result<String> {
    let row = clickhouse
        .query(
            r#"
            SELECT concat(
                toString((
                    SELECT max(indexed_at_unix_ms)
                    FROM ingested_blocks FINAL
                    WHERE chain = 'tron' AND ingestion_status = 'COMPLETE'
                )),
                ':',
                toString((
                    SELECT max(toUnixTimestamp64Milli(created_at))
                    FROM address_entity FINAL
                    WHERE address = ?
                )),
                ':',
                toString((
                    SELECT max(run.completed_at_unix_ms)
                    FROM exposure_runs AS run FINAL
                    INNER JOIN address_exposure AS exposure FINAL
                        ON exposure.source_address = run.source_address
                       AND exposure.propagation_run_id = run.propagation_run_id
                    WHERE exposure.exposed_address = ?
                      AND run.status = 'COMPLETE'
                )),
                ':',
                toString((
                    SELECT max(toUnixTimestamp64Milli(seed.created_at))
                    FROM address_exposure AS exposure FINAL
                    INNER JOIN exposure_seeds AS seed FINAL
                        ON seed.address = exposure.source_address
                    WHERE exposure.exposed_address = ?
                )),
                ':',
                toString((
                    SELECT max(deployed_at_unix_ms)
                    FROM wallet_ml_model_deployments FINAL
                    WHERE environment = 'production'
                      AND feature_schema_version = 'tron_wallet_behavior_features_v2'
                )),
                ':',
                toString((
                    SELECT max(toUnixTimestamp64Milli(metadata.updated_at))
                    FROM wallet_asset_balances AS balance
                    INNER JOIN token_metadata AS metadata FINAL
                        ON balance.asset_type = 'trc20'
                       AND balance.asset_id = metadata.token_address
                    WHERE balance.address = ?
                ))
            ) AS analysis_input_version
            "#,
        )
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .fetch_one::<AnalysisInputVersionRow>()
        .await
        .context("failed to load TRON analysis input version")?;

    Ok(row.analysis_input_version)
}

fn exposure_min_hop_distance(investigation: &WalletInvestigation) -> u8 {
    investigation
        .ai_risk
        .evidence_refs
        .iter()
        .find_map(|item| item.strip_prefix("propagated_exposure_min_hop="))
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or_default()
}

fn persistence(source: &str) -> WalletAnalysisPersistence {
    WalletAnalysisPersistence {
        source: source.to_string(),
        snapshot_table: SNAPSHOT_TABLE.to_string(),
        evidence_table: EVIDENCE_TABLE.to_string(),
        subject_table: SUBJECT_TABLE.to_string(),
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn empty_to_json(value: &str) -> Value {
    if value.trim().is_empty() {
        Value::Null
    } else {
        Value::String(value.to_string())
    }
}

fn related_tx_hash(value: &str) -> Option<String> {
    value
        .split([' ', ',', ';'])
        .find(|part| part.len() >= 32 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|part| part.to_string())
}

fn related_address(value: &str) -> Option<String> {
    value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ':' | ',' | ';' | '=' | '(' | ')'))
        .find(|part| part.starts_with('T') && part.len() >= 30 && part.len() <= 40)
        .map(|part| part.to_string())
}
