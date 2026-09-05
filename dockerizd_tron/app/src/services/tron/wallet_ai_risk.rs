use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::services::tron::risk_math::{clamp01, ratio, risk_level};
use crate::services::tron::wallet_exposure::{WalletExposureSummary, load_wallet_exposure_summary};
use crate::services::tron::wallet_fingerprint::{WalletFingerprint, build_wallet_fingerprint};

const FEATURE_SCHEMA_VERSION: &str = "tron_wallet_behavior_features_v2";
const ML_STATUS_SCORED: &str = "SCORED";
const ML_STATUS_NOT_TRAINED: &str = "MODEL_NOT_TRAINED";
const ML_STATUS_DISABLED: &str = "DISABLED";

const MODEL_FEATURE_NAMES: &[&str] = &[
    "total_transfers_log",
    "unique_transactions_log",
    "incoming_transfers_log",
    "outgoing_transfers_log",
    "unique_senders_log",
    "unique_receivers_log",
    "fan_in_score",
    "fan_out_score",
    "flow_imbalance_score",
    "burst_score",
    "swap_ratio",
    "bridge_ratio",
    "exchange_interaction_ratio",
    "contract_call_ratio",
    "counterparty_concentration",
    "token_diversity_score",
    "exposure_score",
    "exposure_source_count_score",
    "exposure_path_count_score",
    "exposure_min_hop_score",
    "identity_confidence",
    "exchange_service_wallet_score",
    "truncated_sample_score",
    "data_volume_score",
];

#[derive(Debug, Clone, Serialize)]
pub struct WalletAiRiskAssessment {
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction_id: Option<String>,
    pub snapshot_id: String,
    pub address: String,
    pub window_days: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_score: Option<f32>,
    pub risk_level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calibration_version: Option<String>,
    pub feature_schema_version: String,
    pub feature_importance: Vec<MlFeatureContribution>,
    pub model_patterns: Vec<MlPatternSignal>,
    pub evidence_refs: Vec<String>,
    pub feature_snapshot: WalletFeatureSnapshot,
    pub inferred_at_unix_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistence: Option<AiRiskPersistence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletFeatureSnapshot {
    pub snapshot_id: String,
    pub address: String,
    pub window_days: u16,
    pub feature_schema_version: String,
    pub feature_names: Vec<String>,
    pub features: WalletRiskFeatureVector,
    pub evidence_refs: Vec<String>,
    pub generated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletRiskFeatureVector {
    pub total_transfers: u64,
    pub unique_transactions: u64,
    pub incoming_transfers: u64,
    pub outgoing_transfers: u64,
    pub unique_senders: u64,
    pub unique_receivers: u64,
    pub total_transfers_log: f32,
    pub unique_transactions_log: f32,
    pub incoming_transfers_log: f32,
    pub outgoing_transfers_log: f32,
    pub unique_senders_log: f32,
    pub unique_receivers_log: f32,
    pub fan_in_score: f32,
    pub fan_out_score: f32,
    pub flow_imbalance_score: f32,
    pub burst_score: f32,
    pub swap_ratio: f32,
    pub bridge_ratio: f32,
    pub exchange_interaction_ratio: f32,
    pub contract_call_ratio: f32,
    pub counterparty_concentration: f32,
    pub token_diversity_score: f32,
    pub exposure_source_count: u32,
    pub exposure_path_count: u64,
    pub exposure_score: f32,
    pub exposure_source_count_score: f32,
    pub exposure_path_count_score: f32,
    pub exposure_min_hop_score: f32,
    pub identity_confidence: f32,
    pub exchange_service_wallet_score: f32,
    pub truncated_sample_score: f32,
    pub data_volume_score: f32,
}

impl WalletRiskFeatureVector {
    pub fn feature_names() -> Vec<String> {
        MODEL_FEATURE_NAMES
            .iter()
            .map(|feature| (*feature).to_string())
            .collect()
    }

    fn value(&self, feature: &str) -> Option<f32> {
        match feature {
            "total_transfers_log" => Some(self.total_transfers_log),
            "unique_transactions_log" => Some(self.unique_transactions_log),
            "incoming_transfers_log" => Some(self.incoming_transfers_log),
            "outgoing_transfers_log" => Some(self.outgoing_transfers_log),
            "unique_senders_log" => Some(self.unique_senders_log),
            "unique_receivers_log" => Some(self.unique_receivers_log),
            "fan_in_score" => Some(self.fan_in_score),
            "fan_out_score" => Some(self.fan_out_score),
            "flow_imbalance_score" => Some(self.flow_imbalance_score),
            "burst_score" => Some(self.burst_score),
            "swap_ratio" => Some(self.swap_ratio),
            "bridge_ratio" => Some(self.bridge_ratio),
            "exchange_interaction_ratio" => Some(self.exchange_interaction_ratio),
            "contract_call_ratio" => Some(self.contract_call_ratio),
            "counterparty_concentration" => Some(self.counterparty_concentration),
            "token_diversity_score" => Some(self.token_diversity_score),
            "exposure_score" => Some(self.exposure_score),
            "exposure_source_count_score" => Some(self.exposure_source_count_score),
            "exposure_path_count_score" => Some(self.exposure_path_count_score),
            "exposure_min_hop_score" => Some(self.exposure_min_hop_score),
            "identity_confidence" => Some(self.identity_confidence),
            "exchange_service_wallet_score" => Some(self.exchange_service_wallet_score),
            "truncated_sample_score" => Some(self.truncated_sample_score),
            "data_volume_score" => Some(self.data_volume_score),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MlFeatureContribution {
    pub feature: String,
    pub raw_value: f32,
    pub model_value: f32,
    pub coefficient: f32,
    pub contribution: f32,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MlPatternSignal {
    pub pattern: String,
    pub feature: String,
    pub value: f32,
    pub contribution: f32,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiRiskPersistence {
    pub feature_snapshot_table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction_table: Option<String>,
    pub persisted_at_unix_ms: u64,
}

#[derive(Debug, Serialize, clickhouse::Row)]
struct WalletMlFeatureSnapshotInsertRow {
    snapshot_id: String,
    address: String,
    window_days: u16,
    feature_schema_version: String,
    feature_names: Vec<String>,
    features_json: String,
    evidence_refs: Vec<String>,
    generated_at_unix_ms: u64,
}

#[derive(Debug, Serialize, clickhouse::Row)]
struct WalletMlPredictionInsertRow {
    prediction_id: String,
    snapshot_id: String,
    model_id: String,
    model_version: String,
    model_family: String,
    calibration_version: String,
    address: String,
    window_days: u16,
    risk_probability: f32,
    risk_percent: u8,
    risk_level: String,
    confidence: f32,
    feature_importance_json: String,
    model_patterns_json: String,
    evidence_refs: Vec<String>,
    predicted_at_unix_ms: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ActiveWalletMlModelRow {
    model_id: String,
    model_version: String,
    model_family: String,
    feature_schema_version: String,
    calibration_version: String,
    artifact_json: String,
    artifact_sha256: String,
    metrics_json: String,
    model_quality_score: f32,
    trained_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct ActiveWalletMlModel {
    model_id: String,
    model_version: String,
    model_family: String,
    feature_schema_version: String,
    calibration_version: String,
    artifact_json: String,
    artifact_sha256: String,
    metrics_json: String,
    model_quality_score: f32,
    trained_at_unix_ms: u64,
}

impl From<ActiveWalletMlModelRow> for ActiveWalletMlModel {
    fn from(row: ActiveWalletMlModelRow) -> Self {
        Self {
            model_id: row.model_id,
            model_version: row.model_version,
            model_family: row.model_family,
            feature_schema_version: row.feature_schema_version,
            calibration_version: row.calibration_version,
            artifact_json: row.artifact_json,
            artifact_sha256: row.artifact_sha256,
            metrics_json: row.metrics_json,
            model_quality_score: row.model_quality_score,
            trained_at_unix_ms: row.trained_at_unix_ms,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct WalletMlModelArtifact {
    model_type: String,
    feature_names: Vec<String>,
    #[serde(default)]
    intercept: Option<f32>,
    #[serde(default)]
    coefficients: Vec<f32>,
    #[serde(default)]
    feature_means: Vec<f32>,
    #[serde(default)]
    feature_stds: Vec<f32>,
    #[serde(default)]
    hidden_layers: Vec<MlpLayerArtifact>,
    #[serde(default)]
    output_weights: Vec<f32>,
    #[serde(default)]
    output_bias: f32,
    #[serde(default)]
    calibration: Option<WalletMlCalibration>,
    #[serde(default)]
    explanation_top_k: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct MlpLayerArtifact {
    weights: Vec<Vec<f32>>,
    bias: Vec<f32>,
    #[serde(default)]
    activation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WalletMlCalibration {
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    slope: Option<f32>,
    #[serde(default)]
    intercept: Option<f32>,
}

pub async fn build_wallet_ai_risk_assessment(
    clickhouse: Arc<Client>,
    address: &str,
    window_days: Option<u16>,
    top_counterparties: Option<usize>,
    max_events: Option<u64>,
) -> anyhow::Result<WalletAiRiskAssessment> {
    let fingerprint = build_wallet_fingerprint(
        clickhouse.clone(),
        address,
        window_days,
        top_counterparties,
        max_events,
    )
    .await?;
    let exposure = load_wallet_exposure_summary(clickhouse.clone(), address, Some(25)).await?;

    build_and_persist_wallet_ai_risk(clickhouse, &fingerprint, exposure).await
}

pub async fn build_and_persist_wallet_ai_risk(
    clickhouse: Arc<Client>,
    fingerprint: &WalletFingerprint,
    exposure: WalletExposureSummary,
) -> anyhow::Result<WalletAiRiskAssessment> {
    let snapshot = build_wallet_feature_snapshot(fingerprint, &exposure);
    let active_model = load_active_wallet_ml_model(&clickhouse).await?;
    let mut assessment = match active_model {
        Some(model) => infer_wallet_ai_risk_with_model(snapshot, &model)?,
        None => model_not_trained_assessment(snapshot),
    };

    let persistence = persist_wallet_ai_risk_assessment(clickhouse, &assessment).await?;
    assessment.persistence = Some(persistence);

    Ok(assessment)
}

pub fn build_disabled_wallet_ai_risk(
    fingerprint: &WalletFingerprint,
    exposure: WalletExposureSummary,
) -> WalletAiRiskAssessment {
    let snapshot = build_wallet_feature_snapshot(fingerprint, &exposure);
    model_disabled_assessment(snapshot)
}

pub fn build_wallet_feature_snapshot(
    fingerprint: &WalletFingerprint,
    exposure: &WalletExposureSummary,
) -> WalletFeatureSnapshot {
    let generated_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let features = WalletRiskFeatureVector {
        total_transfers: fingerprint.flows.total_transfers,
        unique_transactions: fingerprint.flows.unique_transactions,
        incoming_transfers: fingerprint.flows.incoming_transfers,
        outgoing_transfers: fingerprint.flows.outgoing_transfers,
        unique_senders: fingerprint.flows.unique_senders,
        unique_receivers: fingerprint.flows.unique_receivers,
        total_transfers_log: log_count(fingerprint.flows.total_transfers),
        unique_transactions_log: log_count(fingerprint.flows.unique_transactions),
        incoming_transfers_log: log_count(fingerprint.flows.incoming_transfers),
        outgoing_transfers_log: log_count(fingerprint.flows.outgoing_transfers),
        unique_senders_log: log_count(fingerprint.flows.unique_senders),
        unique_receivers_log: log_count(fingerprint.flows.unique_receivers),
        fan_in_score: share_of_total(
            fingerprint.flows.unique_senders,
            fingerprint.flows.unique_receivers,
        ),
        fan_out_score: share_of_total(
            fingerprint.flows.unique_receivers,
            fingerprint.flows.unique_senders,
        ),
        flow_imbalance_score: flow_imbalance(
            fingerprint.flows.incoming_transfers,
            fingerprint.flows.outgoing_transfers,
        ),
        burst_score: clamp01(fingerprint.behavior.burst_score),
        swap_ratio: clamp01(fingerprint.behavior.swap_ratio),
        bridge_ratio: clamp01(fingerprint.behavior.bridge_ratio),
        exchange_interaction_ratio: clamp01(fingerprint.behavior.exchange_interaction_ratio),
        contract_call_ratio: clamp01(fingerprint.behavior.contract_call_ratio),
        counterparty_concentration: clamp01(fingerprint.behavior.counterparty_concentration),
        token_diversity_score: log_count_score(fingerprint.behavior.token_diversity as u64, 50),
        exposure_source_count: exposure.source_count,
        exposure_path_count: exposure.path_count,
        exposure_score: clamp01(exposure.score),
        exposure_source_count_score: log_count_score(u64::from(exposure.source_count), 25),
        exposure_path_count_score: log_count_score(exposure.path_count, 1_000),
        exposure_min_hop_score: hop_proximity_score(exposure.min_hop_distance),
        identity_confidence: clamp01(fingerprint.identity.confidence),
        exchange_service_wallet_score: if fingerprint.identity.identity_type
            == "exchange_service_wallet"
        {
            1.0
        } else {
            0.0
        },
        truncated_sample_score: if fingerprint.is_truncated { 1.0 } else { 0.0 },
        data_volume_score: log_count_score(fingerprint.flows.total_transfers, 10_000),
    };
    let snapshot_id = wallet_feature_snapshot_id(
        &fingerprint.address,
        FEATURE_SCHEMA_VERSION,
        generated_at_unix_ms,
    );
    let evidence_refs = build_feature_evidence_refs(fingerprint, exposure, &features);

    WalletFeatureSnapshot {
        snapshot_id,
        address: fingerprint.address.clone(),
        window_days: fingerprint.window_days,
        feature_schema_version: FEATURE_SCHEMA_VERSION.to_string(),
        feature_names: WalletRiskFeatureVector::feature_names(),
        features,
        evidence_refs,
        generated_at_unix_ms,
    }
}

async fn load_active_wallet_ml_model(
    clickhouse: &Client,
) -> anyhow::Result<Option<ActiveWalletMlModel>> {
    let deployed = clickhouse
        .query(
            r#"
            SELECT
                registry.model_id,
                registry.model_version,
                registry.model_family,
                registry.feature_schema_version,
                registry.calibration_version,
                registry.artifact_json,
                registry.artifact_sha256,
                registry.metrics_json,
                registry.model_quality_score,
                registry.trained_at_unix_ms
            FROM wallet_ml_model_registry AS registry FINAL
            INNER JOIN wallet_ml_model_deployments AS deployment FINAL
                ON deployment.model_id = registry.model_id
               AND deployment.model_version = registry.model_version
               AND deployment.feature_schema_version = registry.feature_schema_version
            WHERE deployment.environment = 'production'
              AND deployment.status = 'ACTIVE'
              AND deployment.feature_schema_version = ?
            ORDER BY deployment.deployed_at_unix_ms DESC,
                     registry.trained_at_unix_ms DESC
            LIMIT 1
            "#,
        )
        .bind(FEATURE_SCHEMA_VERSION)
        .fetch_optional::<ActiveWalletMlModelRow>()
        .await
        .context("failed to load deployed TRON wallet ML model")?;

    Ok(deployed.map(ActiveWalletMlModel::from))
}

fn infer_wallet_ai_risk_with_model(
    snapshot: WalletFeatureSnapshot,
    model: &ActiveWalletMlModel,
) -> anyhow::Result<WalletAiRiskAssessment> {
    let actual_artifact_sha256 = format!("{:x}", Sha256::digest(model.artifact_json.as_bytes()));
    if !model.artifact_sha256.is_empty() && model.artifact_sha256 != actual_artifact_sha256 {
        return Err(anyhow!(
            "active TRON wallet ML model artifact checksum mismatch"
        ));
    }

    let artifact: WalletMlModelArtifact = serde_json::from_str(&model.artifact_json)
        .context("active TRON wallet ML model artifact is not valid JSON")?;
    validate_artifact(&artifact)?;
    let registered_family = model.model_family.to_ascii_lowercase();
    let artifact_family = artifact.model_type.to_ascii_lowercase();
    let family_matches = registered_family == artifact_family
        || (matches!(
            registered_family.as_str(),
            "logistic_regression" | "binary_logistic_regression"
        ) && matches!(
            artifact_family.as_str(),
            "logistic_regression" | "binary_logistic_regression"
        ));
    if !family_matches {
        return Err(anyhow!(
            "registered model family {} does not match artifact type {}",
            model.model_family,
            artifact.model_type
        ));
    }

    if model.feature_schema_version != snapshot.feature_schema_version {
        return Err(anyhow!(
            "active model feature schema {} does not match snapshot schema {}",
            model.feature_schema_version,
            snapshot.feature_schema_version
        ));
    }

    let inferred_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let evaluation = evaluate_model_artifact(&snapshot, &artifact)?;
    let risk_score = calibrated_probability(evaluation.logit, &artifact);
    let risk_percent = (clamp01(risk_score) * 100.0).round() as u8;
    let risk_level = risk_level(risk_percent).to_string();
    let top_k = artifact.explanation_top_k.unwrap_or(12).clamp(1, 24);
    let mut feature_importance = evaluation.contributions;
    feature_importance.sort_by(|left, right| {
        right
            .contribution
            .abs()
            .partial_cmp(&left.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    feature_importance.truncate(top_k);
    let model_patterns = build_model_patterns(&feature_importance);
    let prediction_id = wallet_ml_prediction_id(
        &snapshot.snapshot_id,
        &model.model_id,
        &model.model_version,
        inferred_at_unix_ms,
    );
    let confidence = if model.model_quality_score > 0.0 {
        Some(prediction_confidence(
            risk_score,
            model.model_quality_score,
            &snapshot,
            &artifact,
        ))
    } else {
        None
    };
    let evidence_refs = build_model_evidence_refs(&snapshot, model);

    Ok(WalletAiRiskAssessment {
        status: ML_STATUS_SCORED.to_string(),
        message: "Scored by the active ClickHouse-registered wallet ML model.".to_string(),
        prediction_id: Some(prediction_id),
        snapshot_id: snapshot.snapshot_id.clone(),
        address: snapshot.address.clone(),
        window_days: snapshot.window_days,
        risk_percent: Some(risk_percent),
        risk_score: Some(risk_score),
        risk_level,
        confidence,
        model_id: Some(model.model_id.clone()),
        model_version: Some(model.model_version.clone()),
        model_family: Some(model.model_family.clone()),
        calibration_version: Some(model.calibration_version.clone()),
        feature_schema_version: snapshot.feature_schema_version.clone(),
        feature_importance,
        model_patterns,
        evidence_refs,
        feature_snapshot: snapshot,
        inferred_at_unix_ms,
        persistence: None,
    })
}

fn model_not_trained_assessment(snapshot: WalletFeatureSnapshot) -> WalletAiRiskAssessment {
    let inferred_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let evidence_refs = snapshot.evidence_refs.clone();

    WalletAiRiskAssessment {
        status: ML_STATUS_NOT_TRAINED.to_string(),
        message: format!(
            "No ACTIVE TRON wallet ML model is registered for feature schema {}. The feature snapshot was persisted for training and later inference.",
            snapshot.feature_schema_version
        ),
        prediction_id: None,
        snapshot_id: snapshot.snapshot_id.clone(),
        address: snapshot.address.clone(),
        window_days: snapshot.window_days,
        risk_percent: None,
        risk_score: None,
        risk_level: "UNAVAILABLE".to_string(),
        confidence: None,
        model_id: None,
        model_version: None,
        model_family: None,
        calibration_version: None,
        feature_schema_version: snapshot.feature_schema_version.clone(),
        feature_importance: Vec::new(),
        model_patterns: Vec::new(),
        evidence_refs,
        feature_snapshot: snapshot,
        inferred_at_unix_ms,
        persistence: None,
    }
}

fn model_disabled_assessment(snapshot: WalletFeatureSnapshot) -> WalletAiRiskAssessment {
    let inferred_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let evidence_refs = snapshot.evidence_refs.clone();

    WalletAiRiskAssessment {
        status: ML_STATUS_DISABLED.to_string(),
        message: "TRON wallet AI risk inference is disabled while graph and evidence workflows are under test."
            .to_string(),
        prediction_id: None,
        snapshot_id: snapshot.snapshot_id.clone(),
        address: snapshot.address.clone(),
        window_days: snapshot.window_days,
        risk_percent: None,
        risk_score: None,
        risk_level: "DISABLED".to_string(),
        confidence: None,
        model_id: None,
        model_version: None,
        model_family: None,
        calibration_version: None,
        feature_schema_version: snapshot.feature_schema_version.clone(),
        feature_importance: Vec::new(),
        model_patterns: Vec::new(),
        evidence_refs,
        feature_snapshot: snapshot,
        inferred_at_unix_ms,
        persistence: None,
    }
}

pub async fn persist_wallet_ai_risk_assessment(
    clickhouse: Arc<Client>,
    assessment: &WalletAiRiskAssessment,
) -> anyhow::Result<AiRiskPersistence> {
    let features_json = serde_json::to_string(&assessment.feature_snapshot.features)?;
    let snapshot_row = WalletMlFeatureSnapshotInsertRow {
        snapshot_id: assessment.feature_snapshot.snapshot_id.clone(),
        address: assessment.feature_snapshot.address.clone(),
        window_days: assessment.feature_snapshot.window_days,
        feature_schema_version: assessment.feature_snapshot.feature_schema_version.clone(),
        feature_names: assessment.feature_snapshot.feature_names.clone(),
        features_json,
        evidence_refs: assessment.feature_snapshot.evidence_refs.clone(),
        generated_at_unix_ms: assessment.feature_snapshot.generated_at_unix_ms,
    };

    let mut snapshot_insert = clickhouse
        .insert::<WalletMlFeatureSnapshotInsertRow>("wallet_ml_feature_snapshots")
        .await
        .context("failed to open wallet ML feature snapshot insert")?;
    snapshot_insert.write(&snapshot_row).await?;
    snapshot_insert.end().await?;

    let mut prediction_table = None;
    if assessment.status == ML_STATUS_SCORED {
        let prediction_row = WalletMlPredictionInsertRow {
            prediction_id: assessment
                .prediction_id
                .clone()
                .context("scored ML assessment is missing prediction_id")?,
            snapshot_id: assessment.snapshot_id.clone(),
            model_id: assessment
                .model_id
                .clone()
                .context("scored ML assessment is missing model_id")?,
            model_version: assessment
                .model_version
                .clone()
                .context("scored ML assessment is missing model_version")?,
            model_family: assessment
                .model_family
                .clone()
                .context("scored ML assessment is missing model_family")?,
            calibration_version: assessment
                .calibration_version
                .clone()
                .unwrap_or_else(|| "uncalibrated".to_string()),
            address: assessment.address.clone(),
            window_days: assessment.window_days,
            risk_probability: assessment.risk_score.unwrap_or_default(),
            risk_percent: assessment.risk_percent.unwrap_or_default(),
            risk_level: assessment.risk_level.clone(),
            confidence: assessment.confidence.unwrap_or_default(),
            feature_importance_json: serde_json::to_string(&assessment.feature_importance)?,
            model_patterns_json: serde_json::to_string(&assessment.model_patterns)?,
            evidence_refs: assessment.evidence_refs.clone(),
            predicted_at_unix_ms: assessment.inferred_at_unix_ms,
        };

        let mut prediction_insert = clickhouse
            .insert::<WalletMlPredictionInsertRow>("wallet_ml_predictions")
            .await
            .context("failed to open wallet ML prediction insert")?;
        prediction_insert.write(&prediction_row).await?;
        prediction_insert.end().await?;
        prediction_table = Some("wallet_ml_predictions".to_string());
    }

    Ok(AiRiskPersistence {
        feature_snapshot_table: "wallet_ml_feature_snapshots".to_string(),
        prediction_table,
        persisted_at_unix_ms: Utc::now().timestamp_millis().max(0) as u64,
    })
}

fn validate_artifact(artifact: &WalletMlModelArtifact) -> anyhow::Result<()> {
    let model_type = artifact.model_type.to_ascii_lowercase();
    if !matches!(
        model_type.as_str(),
        "logistic_regression" | "binary_logistic_regression" | "pytorch_mlp"
    ) {
        return Err(anyhow!(
            "unsupported TRON wallet ML model type: {}",
            artifact.model_type
        ));
    }

    if artifact.feature_names.is_empty() {
        return Err(anyhow!("TRON wallet ML model artifact has no features"));
    }
    let known_features = MODEL_FEATURE_NAMES.iter().copied().collect::<HashSet<_>>();
    let mut unique_features = HashSet::new();
    for feature in &artifact.feature_names {
        if !known_features.contains(feature.as_str()) {
            return Err(anyhow!(
                "TRON wallet ML model artifact requires unknown feature: {feature}"
            ));
        }
        if !unique_features.insert(feature.as_str()) {
            return Err(anyhow!(
                "TRON wallet ML model artifact repeats feature: {feature}"
            ));
        }
    }

    if !artifact.feature_means.is_empty()
        && artifact.feature_means.len() != artifact.feature_names.len()
    {
        return Err(anyhow!(
            "TRON wallet ML model artifact has {} feature means for {} features",
            artifact.feature_means.len(),
            artifact.feature_names.len()
        ));
    }

    if !artifact.feature_stds.is_empty()
        && artifact.feature_stds.len() != artifact.feature_names.len()
    {
        return Err(anyhow!(
            "TRON wallet ML model artifact has {} feature stds for {} features",
            artifact.feature_stds.len(),
            artifact.feature_names.len()
        ));
    }
    if artifact
        .feature_means
        .iter()
        .any(|value| !value.is_finite())
        || artifact
            .feature_stds
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(anyhow!(
            "TRON wallet ML model artifact has invalid normalization values"
        ));
    }
    if let Some(calibration) = &artifact.calibration {
        let method = calibration.method.as_deref().unwrap_or("identity");
        if method.eq_ignore_ascii_case("platt") {
            let slope = calibration.slope.unwrap_or(1.0);
            let intercept = calibration.intercept.unwrap_or_default();
            if !slope.is_finite() || slope <= 0.0 || !intercept.is_finite() {
                return Err(anyhow!(
                    "TRON wallet ML model artifact has invalid Platt calibration"
                ));
            }
        } else if !method.eq_ignore_ascii_case("identity") {
            return Err(anyhow!(
                "unsupported TRON wallet ML calibration method: {method}"
            ));
        }
    }

    match model_type.as_str() {
        "logistic_regression" | "binary_logistic_regression" => {
            validate_logistic_artifact(artifact)
        }
        "pytorch_mlp" => validate_mlp_artifact(artifact),
        _ => unreachable!("unsupported model type is checked above"),
    }
}

fn validate_logistic_artifact(artifact: &WalletMlModelArtifact) -> anyhow::Result<()> {
    if artifact.intercept.is_none() {
        return Err(anyhow!(
            "TRON wallet logistic model artifact is missing intercept"
        ));
    }

    if artifact.feature_names.len() != artifact.coefficients.len() {
        return Err(anyhow!(
            "TRON wallet ML model artifact has {} features but {} coefficients",
            artifact.feature_names.len(),
            artifact.coefficients.len()
        ));
    }
    if !artifact.intercept.unwrap_or_default().is_finite()
        || artifact.coefficients.iter().any(|value| !value.is_finite())
    {
        return Err(anyhow!(
            "TRON wallet logistic model artifact has non-finite parameters"
        ));
    }

    Ok(())
}

fn validate_mlp_artifact(artifact: &WalletMlModelArtifact) -> anyhow::Result<()> {
    if artifact.hidden_layers.is_empty() {
        return Err(anyhow!(
            "TRON wallet PyTorch MLP artifact has no hidden layers"
        ));
    }

    let mut expected_input_width = artifact.feature_names.len();
    for (index, layer) in artifact.hidden_layers.iter().enumerate() {
        if layer.weights.is_empty() {
            return Err(anyhow!("MLP layer {index} has no weight rows"));
        }

        if layer.weights.len() != layer.bias.len() {
            return Err(anyhow!(
                "MLP layer {index} has {} output rows but {} bias values",
                layer.weights.len(),
                layer.bias.len()
            ));
        }

        for row in &layer.weights {
            if row.len() != expected_input_width {
                return Err(anyhow!(
                    "MLP layer {index} expects input width {} but found weight row width {}",
                    expected_input_width,
                    row.len()
                ));
            }
            if row.iter().any(|value| !value.is_finite()) {
                return Err(anyhow!("MLP layer {index} has non-finite weights"));
            }
        }
        if layer.bias.iter().any(|value| !value.is_finite()) {
            return Err(anyhow!("MLP layer {index} has non-finite bias values"));
        }

        validate_activation(layer.activation.as_deref().unwrap_or("relu"))?;
        expected_input_width = layer.bias.len();
    }

    if artifact.output_weights.len() != expected_input_width {
        return Err(anyhow!(
            "MLP output layer expects {} weights but found {}",
            expected_input_width,
            artifact.output_weights.len()
        ));
    }
    if artifact
        .output_weights
        .iter()
        .any(|value| !value.is_finite())
        || !artifact.output_bias.is_finite()
    {
        return Err(anyhow!("MLP output layer has non-finite parameters"));
    }

    Ok(())
}

fn validate_activation(activation: &str) -> anyhow::Result<()> {
    if matches!(
        activation.to_ascii_lowercase().as_str(),
        "relu" | "tanh" | "sigmoid" | "identity" | "linear"
    ) {
        Ok(())
    } else {
        Err(anyhow!("unsupported MLP activation: {activation}"))
    }
}

struct ModelEvaluation {
    logit: f32,
    contributions: Vec<MlFeatureContribution>,
}

fn evaluate_model_artifact(
    snapshot: &WalletFeatureSnapshot,
    artifact: &WalletMlModelArtifact,
) -> anyhow::Result<ModelEvaluation> {
    match artifact.model_type.to_ascii_lowercase().as_str() {
        "logistic_regression" | "binary_logistic_regression" => {
            evaluate_logistic_model(snapshot, artifact)
        }
        "pytorch_mlp" => evaluate_mlp_model(snapshot, artifact),
        _ => Err(anyhow!(
            "unsupported TRON wallet ML model type: {}",
            artifact.model_type
        )),
    }
}

fn evaluate_logistic_model(
    snapshot: &WalletFeatureSnapshot,
    artifact: &WalletMlModelArtifact,
) -> anyhow::Result<ModelEvaluation> {
    let mut logit = artifact
        .intercept
        .context("TRON wallet logistic model artifact is missing intercept")?;
    let mut contributions = Vec::with_capacity(artifact.feature_names.len());

    for (index, feature_name) in artifact.feature_names.iter().enumerate() {
        let raw_value = snapshot
            .features
            .value(feature_name)
            .ok_or_else(|| anyhow!("model requires missing feature: {feature_name}"))?;
        let model_value = model_feature_value(raw_value, artifact, index);
        let coefficient = artifact.coefficients[index];
        let contribution = model_value * coefficient;
        logit += contribution;

        contributions.push(MlFeatureContribution {
            feature: feature_name.clone(),
            raw_value,
            model_value,
            coefficient,
            contribution,
            direction: contribution_direction(contribution),
        });
    }

    Ok(ModelEvaluation {
        logit,
        contributions,
    })
}

fn evaluate_mlp_model(
    snapshot: &WalletFeatureSnapshot,
    artifact: &WalletMlModelArtifact,
) -> anyhow::Result<ModelEvaluation> {
    let model_values = model_feature_values(snapshot, artifact)?;
    let logit = mlp_logit(&model_values, artifact)?;
    let mut contributions = Vec::with_capacity(artifact.feature_names.len());

    for (index, feature_name) in artifact.feature_names.iter().enumerate() {
        let mut occluded = model_values.clone();
        occluded[index] = 0.0;
        let occluded_logit = mlp_logit(&occluded, artifact)?;
        let contribution = logit - occluded_logit;
        let raw_value = snapshot
            .features
            .value(feature_name)
            .ok_or_else(|| anyhow!("model requires missing feature: {feature_name}"))?;

        contributions.push(MlFeatureContribution {
            feature: feature_name.clone(),
            raw_value,
            model_value: model_values[index],
            coefficient: if model_values[index].abs() <= f32::EPSILON {
                0.0
            } else {
                contribution / model_values[index]
            },
            contribution,
            direction: contribution_direction(contribution),
        });
    }

    Ok(ModelEvaluation {
        logit,
        contributions,
    })
}

fn model_feature_values(
    snapshot: &WalletFeatureSnapshot,
    artifact: &WalletMlModelArtifact,
) -> anyhow::Result<Vec<f32>> {
    artifact
        .feature_names
        .iter()
        .enumerate()
        .map(|(index, feature_name)| {
            let raw_value = snapshot
                .features
                .value(feature_name)
                .ok_or_else(|| anyhow!("model requires missing feature: {feature_name}"))?;

            Ok(model_feature_value(raw_value, artifact, index))
        })
        .collect()
}

fn mlp_logit(inputs: &[f32], artifact: &WalletMlModelArtifact) -> anyhow::Result<f32> {
    let mut activations = inputs.to_vec();

    for layer in &artifact.hidden_layers {
        activations = dense_layer_forward(&activations, layer)?;
    }

    Ok(dot(&artifact.output_weights, &activations)? + artifact.output_bias)
}

fn dense_layer_forward(inputs: &[f32], layer: &MlpLayerArtifact) -> anyhow::Result<Vec<f32>> {
    let activation = layer.activation.as_deref().unwrap_or("relu");
    layer
        .weights
        .iter()
        .zip(&layer.bias)
        .map(|(weights, bias)| {
            let value = dot(weights, inputs)? + *bias;
            Ok(apply_activation(value, activation))
        })
        .collect()
}

fn dot(weights: &[f32], inputs: &[f32]) -> anyhow::Result<f32> {
    if weights.len() != inputs.len() {
        return Err(anyhow!(
            "model layer width mismatch: {} weights for {} inputs",
            weights.len(),
            inputs.len()
        ));
    }

    Ok(weights
        .iter()
        .zip(inputs)
        .map(|(weight, input)| weight * input)
        .sum())
}

fn apply_activation(value: f32, activation: &str) -> f32 {
    match activation.to_ascii_lowercase().as_str() {
        "relu" => value.max(0.0),
        "tanh" => value.tanh(),
        "sigmoid" => sigmoid(value),
        "identity" | "linear" => value,
        _ => value,
    }
}

fn model_feature_value(raw_value: f32, artifact: &WalletMlModelArtifact, index: usize) -> f32 {
    if artifact.feature_means.is_empty() || artifact.feature_stds.is_empty() {
        return raw_value;
    }

    let std = artifact.feature_stds[index];
    if std.abs() <= f32::EPSILON {
        raw_value - artifact.feature_means[index]
    } else {
        (raw_value - artifact.feature_means[index]) / std
    }
}

fn calibrated_probability(raw_logit: f32, artifact: &WalletMlModelArtifact) -> f32 {
    match &artifact.calibration {
        Some(calibration)
            if calibration
                .method
                .as_deref()
                .unwrap_or("identity")
                .eq_ignore_ascii_case("platt") =>
        {
            sigmoid(
                raw_logit * calibration.slope.unwrap_or(1.0)
                    + calibration.intercept.unwrap_or_default(),
            )
        }
        _ => sigmoid(raw_logit),
    }
}

fn prediction_confidence(
    probability: f32,
    model_quality_score: f32,
    snapshot: &WalletFeatureSnapshot,
    artifact: &WalletMlModelArtifact,
) -> f32 {
    let margin = ((probability - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    let data_support = (snapshot.features.data_volume_score
        * (1.0 - snapshot.features.truncated_sample_score * 0.5))
        .clamp(0.05, 1.0);
    let max_standardized_distance = artifact
        .feature_names
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| {
            snapshot
                .features
                .value(feature)
                .map(|value| model_feature_value(value, artifact, index).abs())
        })
        .fold(0.0_f32, f32::max);
    let distribution_support = if max_standardized_distance <= 3.0 {
        1.0
    } else {
        (1.0 / (1.0 + max_standardized_distance - 3.0)).clamp(0.1, 1.0)
    };

    clamp01(
        model_quality_score.clamp(0.0, 1.0)
            * data_support.sqrt()
            * (0.35 + 0.65 * margin)
            * distribution_support,
    )
}

fn sigmoid(value: f32) -> f32 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    }
}

fn contribution_direction(contribution: f32) -> String {
    if contribution >= 0.0 {
        "raises_risk".to_string()
    } else {
        "lowers_risk".to_string()
    }
}

fn build_model_patterns(contributions: &[MlFeatureContribution]) -> Vec<MlPatternSignal> {
    contributions
        .iter()
        .filter(|item| item.contribution.abs() > 0.0001)
        .take(8)
        .map(|item| MlPatternSignal {
            pattern: pattern_name_for_feature(&item.feature).to_string(),
            feature: item.feature.clone(),
            value: item.raw_value,
            contribution: item.contribution,
            direction: item.direction.clone(),
        })
        .collect()
}

fn pattern_name_for_feature(feature: &str) -> &'static str {
    match feature {
        "swap_ratio" => "swap_activity",
        "bridge_ratio" => "bridge_activity",
        "exchange_interaction_ratio" => "exchange_interaction",
        "counterparty_concentration" => "counterparty_concentration",
        "fan_in_score" => "fan_in_flow_shape",
        "fan_out_score" => "fan_out_flow_shape",
        "flow_imbalance_score" => "directional_flow_imbalance",
        "burst_score" => "bursty_transfer_timing",
        "exposure_score"
        | "exposure_source_count_score"
        | "exposure_path_count_score"
        | "exposure_min_hop_score" => "propagated_exposure",
        "token_diversity_score" => "token_diversity",
        "contract_call_ratio" => "contract_interaction",
        "exchange_service_wallet_score" => "known_service_context",
        "truncated_sample_score" => "incomplete_observation_window",
        _ => "wallet_behavior_feature",
    }
}

fn build_feature_evidence_refs(
    fingerprint: &WalletFingerprint,
    exposure: &WalletExposureSummary,
    features: &WalletRiskFeatureVector,
) -> Vec<String> {
    let mut refs = vec![
        format!(
            "wallet_fingerprint_generated_at:{}",
            fingerprint.generated_at_unix_ms
        ),
        format!("feature:total_transfers={}", features.total_transfers),
        format!("feature:unique_senders={}", features.unique_senders),
        format!("feature:unique_receivers={}", features.unique_receivers),
        format!("feature:swap_ratio={:.3}", features.swap_ratio),
        format!("feature:bridge_ratio={:.3}", features.bridge_ratio),
        format!(
            "feature:exchange_interaction_ratio={:.3}",
            features.exchange_interaction_ratio
        ),
        format!("feature:exposure_score={:.3}", features.exposure_score),
        format!("feature:exposure_source_count={}", exposure.source_count),
        format!("feature:exposure_path_count={}", exposure.path_count),
    ];

    refs.extend(exposure.evidence.iter().take(40).cloned());
    refs.extend(fingerprint.evidence.iter().take(40).cloned());
    refs
}

fn build_model_evidence_refs(
    snapshot: &WalletFeatureSnapshot,
    model: &ActiveWalletMlModel,
) -> Vec<String> {
    let mut refs = snapshot
        .evidence_refs
        .iter()
        .take(80)
        .cloned()
        .collect::<Vec<_>>();
    refs.push(format!("ml_model_id:{}", model.model_id));
    refs.push(format!("ml_model_version:{}", model.model_version));
    refs.push(format!("ml_model_trained_at:{}", model.trained_at_unix_ms));
    if !model.metrics_json.trim().is_empty() {
        refs.push("ml_model_metrics:wallet_ml_model_registry.metrics_json".to_string());
    }
    refs
}

fn log_count(count: u64) -> f32 {
    ((count as f32) + 1.0).ln()
}

fn log_count_score(count: u64, cap: u64) -> f32 {
    let cap = cap.max(1);
    clamp01(log_count(count) / log_count(cap))
}

fn share_of_total(value: u64, other: u64) -> f32 {
    ratio(value as f32, (value + other) as f32)
}

fn flow_imbalance(incoming: u64, outgoing: u64) -> f32 {
    let total = incoming + outgoing;
    if total == 0 {
        return 0.0;
    }

    ((incoming as f32 - outgoing as f32).abs()) / total as f32
}

fn hop_proximity_score(min_hop: Option<u8>) -> f32 {
    match min_hop {
        Some(1) => 1.0,
        Some(2) => 0.75,
        Some(3) => 0.50,
        Some(4) => 0.25,
        Some(_) => 0.10,
        None => 0.0,
    }
}

fn wallet_feature_snapshot_id(
    address: &str,
    feature_schema_version: &str,
    generated_at_unix_ms: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(address.as_bytes());
    hasher.update(b":");
    hasher.update(feature_schema_version.as_bytes());
    hasher.update(b":");
    hasher.update(generated_at_unix_ms.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

fn wallet_ml_prediction_id(
    snapshot_id: &str,
    model_id: &str,
    model_version: &str,
    inferred_at_unix_ms: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(snapshot_id.as_bytes());
    hasher.update(b":");
    hasher.update(model_id.as_bytes());
    hasher.update(b":");
    hasher.update(model_version.as_bytes());
    hasher.update(b":");
    hasher.update(inferred_at_unix_ms.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(features: WalletRiskFeatureVector) -> WalletFeatureSnapshot {
        WalletFeatureSnapshot {
            snapshot_id: "snapshot".to_string(),
            address: "TWallet".to_string(),
            window_days: 90,
            feature_schema_version: FEATURE_SCHEMA_VERSION.to_string(),
            feature_names: WalletRiskFeatureVector::feature_names(),
            features,
            evidence_refs: Vec::new(),
            generated_at_unix_ms: 1,
        }
    }

    fn features() -> WalletRiskFeatureVector {
        WalletRiskFeatureVector {
            total_transfers: 120,
            unique_transactions: 100,
            incoming_transfers: 70,
            outgoing_transfers: 50,
            unique_senders: 55,
            unique_receivers: 10,
            total_transfers_log: log_count(120),
            unique_transactions_log: log_count(100),
            incoming_transfers_log: log_count(70),
            outgoing_transfers_log: log_count(50),
            unique_senders_log: log_count(55),
            unique_receivers_log: log_count(10),
            fan_in_score: share_of_total(55, 10),
            fan_out_score: share_of_total(10, 55),
            flow_imbalance_score: flow_imbalance(70, 50),
            burst_score: 0.70,
            swap_ratio: 0.45,
            bridge_ratio: 0.25,
            exchange_interaction_ratio: 0.35,
            contract_call_ratio: 0.60,
            counterparty_concentration: 0.78,
            token_diversity_score: log_count_score(8, 50),
            exposure_source_count: 4,
            exposure_path_count: 25,
            exposure_score: 0.80,
            exposure_source_count_score: log_count_score(4, 25),
            exposure_path_count_score: log_count_score(25, 1_000),
            exposure_min_hop_score: 0.75,
            identity_confidence: 0.65,
            exchange_service_wallet_score: 0.0,
            truncated_sample_score: 0.0,
            data_volume_score: log_count_score(120, 10_000),
        }
    }

    fn model(
        coefficients: Vec<f32>,
        feature_names: Vec<&str>,
        intercept: f32,
    ) -> ActiveWalletMlModel {
        let artifact = serde_json::json!({
            "model_type": "logistic_regression",
            "feature_names": feature_names,
            "intercept": intercept,
            "coefficients": coefficients,
            "explanation_top_k": 8
        });

        ActiveWalletMlModel {
            model_id: "model".to_string(),
            model_version: "v1".to_string(),
            model_family: "logistic_regression".to_string(),
            feature_schema_version: FEATURE_SCHEMA_VERSION.to_string(),
            calibration_version: "calibration_v1".to_string(),
            artifact_json: artifact.to_string(),
            artifact_sha256: String::new(),
            metrics_json: "{}".to_string(),
            model_quality_score: 0.82,
            trained_at_unix_ms: 1,
        }
    }

    fn mlp_model() -> ActiveWalletMlModel {
        let artifact = serde_json::json!({
            "model_type": "pytorch_mlp",
            "feature_names": [
                "exposure_score",
                "bridge_ratio",
                "swap_ratio",
                "fan_in_score"
            ],
            "feature_means": [0.0, 0.0, 0.0, 0.0],
            "feature_stds": [1.0, 1.0, 1.0, 1.0],
            "hidden_layers": [
                {
                    "activation": "relu",
                    "weights": [
                        [2.0, 1.5, 1.0, 0.5],
                        [-1.0, 0.0, 0.0, 0.0]
                    ],
                    "bias": [0.0, 0.0]
                }
            ],
            "output_weights": [2.5, -0.5],
            "output_bias": -1.0,
            "explanation_top_k": 8
        });

        ActiveWalletMlModel {
            model_id: "pytorch_mlp".to_string(),
            model_version: "v1".to_string(),
            model_family: "pytorch_mlp".to_string(),
            feature_schema_version: FEATURE_SCHEMA_VERSION.to_string(),
            calibration_version: "calibration_v1".to_string(),
            artifact_json: artifact.to_string(),
            artifact_sha256: String::new(),
            metrics_json: "{}".to_string(),
            model_quality_score: 0.82,
            trained_at_unix_ms: 1,
        }
    }

    #[test]
    fn no_registered_model_returns_not_trained_status() {
        let assessment = model_not_trained_assessment(snapshot(features()));

        assert_eq!(assessment.status, ML_STATUS_NOT_TRAINED);
        assert!(assessment.risk_percent.is_none());
        assert_eq!(assessment.risk_level, "UNAVAILABLE");
    }

    #[test]
    fn learned_logistic_model_outputs_probability() {
        let model = model(
            vec![3.0, 2.0, 1.5, 1.0],
            vec![
                "exposure_score",
                "bridge_ratio",
                "swap_ratio",
                "fan_in_score",
            ],
            -2.0,
        );

        let assessment = infer_wallet_ai_risk_with_model(snapshot(features()), &model)
            .expect("model inference should succeed");

        assert_eq!(assessment.status, ML_STATUS_SCORED);
        assert!(assessment.risk_percent.unwrap_or_default() >= 80);
        assert!(
            assessment
                .model_patterns
                .iter()
                .any(|pattern| pattern.pattern == "propagated_exposure")
        );
    }

    #[test]
    fn pytorch_mlp_artifact_outputs_probability() {
        let assessment = infer_wallet_ai_risk_with_model(snapshot(features()), &mlp_model())
            .expect("pytorch mlp artifact inference should succeed");

        assert_eq!(assessment.status, ML_STATUS_SCORED);
        assert_eq!(assessment.model_family.as_deref(), Some("pytorch_mlp"));
        assert!(assessment.risk_percent.unwrap_or_default() >= 80);
        assert!(
            assessment
                .feature_importance
                .iter()
                .any(|item| item.feature == "exposure_score")
        );
    }

    #[test]
    fn model_artifact_must_match_feature_shape() {
        let model = model(vec![1.0], vec!["bridge_ratio", "swap_ratio"], 0.0);

        let err = infer_wallet_ai_risk_with_model(snapshot(features()), &model)
            .expect_err("invalid artifact should fail");

        assert!(format!("{err:#}").contains("features but"));
    }

    #[test]
    fn registered_artifact_checksum_is_enforced() {
        let mut model = mlp_model();
        model.artifact_sha256 = "not-the-artifact-checksum".to_string();

        let err = infer_wallet_ai_risk_with_model(snapshot(features()), &model)
            .expect_err("checksum mismatch must fail inference");

        assert!(format!("{err:#}").contains("checksum mismatch"));
    }
}
