use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use clickhouse::{Client, Row, RowOwned, RowWrite};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{config::AppConfig, db::database_client};

const SCORING_METHOD: &str = "VERSIONED_EVIDENCE_POLICY";

#[derive(Clone)]
pub struct EvidenceRiskEngine {
    client: Client,
    network_id: String,
    policy_version: String,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskSignalOutput {
    pub signal_type: String,
    pub signal_strength: f64,
    pub contribution: f64,
    pub summary: String,
    pub evidence_refs: Vec<String>,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskAssessmentOutput {
    pub enabled: bool,
    pub status: String,
    pub assessment_id: Option<String>,
    pub policy_version: String,
    pub scoring_method: String,
    pub risk_score: Option<f64>,
    pub risk_level: Option<String>,
    pub signals: Vec<RiskSignalOutput>,
    pub top_reasons: Vec<String>,
    pub exposure_run_id: Option<String>,
    pub as_of_block: Option<u64>,
    pub probability_claimed: bool,
}

#[derive(Debug, Deserialize, Row)]
struct PolicyRow {
    medium_threshold: f64,
    high_threshold: f64,
}

#[derive(Debug, Deserialize, Row)]
struct RuleRow {
    signal_type: String,
    max_contribution: f64,
    description: String,
}

#[derive(Debug, Deserialize, Row)]
struct CheckpointRow {
    last_synced_block: u64,
}

#[derive(Debug, Deserialize, Row)]
struct EntityEvidenceRow {
    risk_level: u8,
    is_exposure_seed: u8,
    entity_ids: Vec<String>,
    evidence_refs: Vec<String>,
}

#[derive(Debug, Deserialize, Row)]
struct ExposurePathRow {
    path_id: String,
    direction: String,
    hop_count: u8,
    exposure_score: f64,
    seed_address: String,
    seed_entity_id: String,
    service_mediated: u8,
    relationship_ids: Vec<String>,
    tx_hashes: Vec<String>,
    path_addresses: Vec<String>,
}

#[derive(Debug, Deserialize, Row)]
struct SemanticEvidenceRow {
    event_id: String,
    tx_hash: String,
    event_type: String,
    block_timestamp_unix_ms: u64,
}

#[derive(Debug, Deserialize, Row)]
struct PassThroughRow {
    pair_count: u64,
    evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Row, Serialize)]
struct RiskSignalRow {
    signal_id: String,
    assessment_id: String,
    network_id: String,
    address: String,
    policy_version: String,
    signal_type: String,
    signal_strength: f64,
    contribution: f64,
    summary: String,
    evidence_refs: Vec<String>,
    evidence_json: String,
    as_of_block: u64,
    created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Row, Serialize)]
struct RiskAssessmentRow {
    assessment_id: String,
    network_id: String,
    address: String,
    policy_version: String,
    risk_score: f64,
    risk_level: String,
    signal_count: u16,
    top_reasons: Vec<String>,
    evidence_refs: Vec<String>,
    exposure_run_id: String,
    as_of_block: u64,
    as_of_unix_ms: u64,
    scoring_method: String,
    created_at_unix_ms: u64,
}

impl EvidenceRiskEngine {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            client: database_client(config),
            network_id: config.eth_network_id.clone(),
            policy_version: config.eth_risk_policy_version.clone(),
            enabled: config.eth_risk_engine_enabled,
        }
    }

    pub async fn assess(&self, address: &str) -> anyhow::Result<RiskAssessmentOutput> {
        if !self.enabled {
            return Ok(RiskAssessmentOutput {
                enabled: false,
                status: "disabled".to_string(),
                assessment_id: None,
                policy_version: self.policy_version.clone(),
                scoring_method: SCORING_METHOD.to_string(),
                risk_score: None,
                risk_level: None,
                signals: Vec::new(),
                top_reasons: Vec::new(),
                exposure_run_id: None,
                as_of_block: None,
                probability_claimed: false,
            });
        }

        let policy = self.load_policy().await?;
        let rules = self.load_rules().await?;
        let as_of_block = self.load_checkpoint().await?;
        let exposure_run_id = self.load_exposure_run().await?;
        let now = unix_time_millis()?;
        let assessment_id = stable_id(
            "eth_risk_assessment",
            &[
                &self.network_id,
                address,
                &self.policy_version,
                &as_of_block.to_string(),
                exposure_run_id.as_deref().unwrap_or("no_exposure_run"),
            ],
        );

        let entity = self.load_entity_evidence(address).await?;
        let exposure = self
            .load_exposure_paths(address, exposure_run_id.as_deref())
            .await?;
        let semantic = self.load_semantic_evidence(address).await?;
        let pass_through = self.load_pass_through(address).await?;

        let mut signals = Vec::new();
        if let Some(entity) = entity {
            if entity.is_exposure_seed == 1 {
                push_signal(
                    &mut signals,
                    &rules,
                    "CONFIRMED_ILLICIT_SEED",
                    f64::from(entity.risk_level) / 100.0,
                    entity.evidence_refs,
                    serde_json::json!({
                        "entity_ids": entity.entity_ids,
                        "risk_level": entity.risk_level
                    }),
                );
            }
        }

        let direct_received =
            strongest_exposure(&exposure, "RECEIVED_FROM_SEED", |path| path.hop_count == 1);
        if let Some(path) = direct_received {
            push_exposure_signal(&mut signals, &rules, "DIRECT_RECEIVED_EXPOSURE", path);
        }
        let direct_sent = strongest_exposure(&exposure, "SENT_TO_SEED", |path| path.hop_count == 1);
        if let Some(path) = direct_sent {
            push_exposure_signal(&mut signals, &rules, "DIRECT_SENT_EXPOSURE", path);
        }
        let indirect = exposure
            .iter()
            .filter(|path| path.hop_count > 1)
            .max_by(|left, right| left.exposure_score.total_cmp(&right.exposure_score));
        if let Some(path) = indirect {
            push_exposure_signal(&mut signals, &rules, "INDIRECT_EXPOSURE", path);
        }

        let mixer_events = semantic
            .iter()
            .filter(|event| event.event_type.starts_with("mixer_"))
            .collect::<Vec<_>>();
        if !mixer_events.is_empty() {
            push_signal(
                &mut signals,
                &rules,
                "MIXER_INTERACTION",
                (mixer_events.len() as f64 / 2.0).min(1.0),
                mixer_events
                    .iter()
                    .map(|event| event.event_id.clone())
                    .collect(),
                serde_json::json!({
                    "event_count": mixer_events.len(),
                    "tx_hashes": mixer_events.iter().map(|event| &event.tx_hash).collect::<Vec<_>>()
                }),
            );
        }

        if let Some(refs) = nearby_semantic_sequence(
            &semantic,
            |event| event.event_type.starts_with("mixer_"),
            |event| event.event_type == "bridge_transfer",
            3_600_000,
        ) {
            push_signal(
                &mut signals,
                &rules,
                "MIXER_BRIDGE_SEQUENCE",
                1.0,
                refs,
                serde_json::json!({"window_seconds": 3600}),
            );
        }
        if let Some(refs) = nearby_semantic_sequence(
            &semantic,
            |event| event.event_type == "swap",
            |event| event.event_type == "bridge_transfer",
            3_600_000,
        ) {
            push_signal(
                &mut signals,
                &rules,
                "RAPID_SWAP_BRIDGE_SEQUENCE",
                1.0,
                refs,
                serde_json::json!({"window_seconds": 3600}),
            );
        }
        if pass_through.pair_count > 0 {
            push_signal(
                &mut signals,
                &rules,
                "RAPID_PASS_THROUGH",
                (pass_through.pair_count as f64 / 3.0).min(1.0),
                pass_through.evidence_refs,
                serde_json::json!({
                    "pair_count": pass_through.pair_count,
                    "window_seconds": 600
                }),
            );
        }

        signals.sort_by(|left, right| right.contribution.total_cmp(&left.contribution));
        let risk_score = signals
            .iter()
            .map(|signal| signal.contribution)
            .sum::<f64>()
            .clamp(0.0, 100.0);
        let risk_level = if risk_score >= policy.high_threshold {
            "HIGH"
        } else if risk_score >= policy.medium_threshold {
            "MEDIUM"
        } else if risk_score > 0.0 {
            "LOW"
        } else {
            "NONE"
        }
        .to_string();
        let top_reasons = signals
            .iter()
            .take(5)
            .map(|signal| signal.summary.clone())
            .collect::<Vec<_>>();
        let evidence_refs = signals
            .iter()
            .flat_map(|signal| signal.evidence_refs.iter().cloned())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        self.persist(
            address,
            &assessment_id,
            &signals,
            risk_score,
            &risk_level,
            &top_reasons,
            &evidence_refs,
            exposure_run_id.as_deref().unwrap_or(""),
            as_of_block,
            now,
        )
        .await?;

        Ok(RiskAssessmentOutput {
            enabled: true,
            status: "available".to_string(),
            assessment_id: Some(assessment_id),
            policy_version: self.policy_version.clone(),
            scoring_method: SCORING_METHOD.to_string(),
            risk_score: Some(risk_score),
            risk_level: Some(risk_level),
            signals,
            top_reasons,
            exposure_run_id,
            as_of_block: Some(as_of_block),
            probability_claimed: false,
        })
    }

    async fn load_policy(&self) -> anyhow::Result<PolicyRow> {
        self.client
            .query(
                r#"
                SELECT medium_threshold, high_threshold
                FROM risk_policies FINAL
                WHERE network_id = ? AND policy_version = ? AND enabled = 1
                LIMIT 1
                "#,
            )
            .bind(&self.network_id)
            .bind(&self.policy_version)
            .fetch_one::<PolicyRow>()
            .await
            .context("active Ethereum risk policy was not found")
    }

    async fn load_rules(&self) -> anyhow::Result<HashMap<String, RuleRow>> {
        let rows = self
            .client
            .query(
                r#"
                SELECT signal_type, max_contribution, description
                FROM risk_policy_rules FINAL
                WHERE network_id = ? AND policy_version = ? AND enabled = 1
                "#,
            )
            .bind(&self.network_id)
            .bind(&self.policy_version)
            .fetch_all::<RuleRow>()
            .await
            .context("failed to load Ethereum risk policy rules")?;
        ensure!(!rows.is_empty(), "active Ethereum risk policy has no rules");
        Ok(rows
            .into_iter()
            .map(|row| (row.signal_type.clone(), row))
            .collect())
    }

    async fn load_checkpoint(&self) -> anyhow::Result<u64> {
        Ok(self
            .client
            .query(
                r#"
                SELECT last_synced_block
                FROM sync_state
                WHERE network_id = ?
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
            )
            .bind(&self.network_id)
            .fetch_optional::<CheckpointRow>()
            .await?
            .map(|row| row.last_synced_block)
            .unwrap_or_default())
    }

    async fn load_exposure_run(&self) -> anyhow::Result<Option<String>> {
        crate::exposure::latest_complete_run(&self.client, &self.network_id).await
    }

    async fn load_entity_evidence(
        &self,
        address: &str,
    ) -> anyhow::Result<Option<EntityEvidenceRow>> {
        self.client
            .query(
                r#"
                SELECT
                    max(risk_level) AS risk_level,
                    max(is_exposure_seed) AS is_exposure_seed,
                    groupUniqArray(entity_id) AS entity_ids,
                    groupUniqArray(source_label_id) AS evidence_refs
                FROM address_entities_active
                WHERE network_id = ? AND address = ?
                HAVING count() > 0
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .fetch_optional::<EntityEvidenceRow>()
            .await
            .context("failed to load Ethereum entity risk evidence")
    }

    async fn load_exposure_paths(
        &self,
        address: &str,
        run_id: Option<&str>,
    ) -> anyhow::Result<Vec<ExposurePathRow>> {
        let Some(run_id) = run_id else {
            return Ok(Vec::new());
        };
        self.client
            .query(
                r#"
                SELECT
                    path_id, direction, hop_count, exposure_score,
                    seed_address, seed_entity_id, service_mediated,
                    relationship_ids, tx_hashes, path_addresses
                FROM address_exposure_best_paths
                WHERE network_id = ? AND run_id = ? AND subject_address = ?
                  AND direction != 'SEED'
                  AND (seed_address, seed_entity_id) IN
                  (
                      SELECT address, entity_id FROM address_entities_active
                      WHERE network_id = ? AND is_exposure_seed = 1 AND risk_level > 0
                  )
                ORDER BY exposure_score DESC
                LIMIT 100
                "#,
            )
            .bind(&self.network_id)
            .bind(run_id)
            .bind(address)
            .bind(&self.network_id)
            .fetch_all::<ExposurePathRow>()
            .await
            .context("failed to load Ethereum exposure evidence")
    }

    async fn load_semantic_evidence(
        &self,
        address: &str,
    ) -> anyhow::Result<Vec<SemanticEvidenceRow>> {
        self.client
            .query(
                r#"
                SELECT event_id, tx_hash, event_type, block_timestamp_unix_ms
                FROM semantic_aml_events_canonical
                WHERE network_id = ? AND subject_address = ?
                ORDER BY block_timestamp_unix_ms
                LIMIT 1000
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .fetch_all::<SemanticEvidenceRow>()
            .await
            .context("failed to load Ethereum semantic risk evidence")
    }

    async fn load_pass_through(&self, address: &str) -> anyhow::Result<PassThroughRow> {
        self.client
            .query(
                r#"
                SELECT
                    count() AS pair_count,
                    groupUniqArray(20)(concat(inbound.tx_hash, ':', outbound.tx_hash))
                        AS evidence_refs
                FROM
                (
                    SELECT tx_hash, asset_id, block_timestamp_unix_ms
                    FROM address_relationships_canonical
                    WHERE network_id = ? AND to_address = ?
                    ORDER BY block_timestamp_unix_ms DESC
                    LIMIT 5000
                ) AS inbound
                INNER JOIN
                (
                    SELECT tx_hash, asset_id, block_timestamp_unix_ms
                    FROM address_relationships_canonical
                    WHERE network_id = ? AND from_address = ?
                    ORDER BY block_timestamp_unix_ms DESC
                    LIMIT 5000
                ) AS outbound
                ON inbound.asset_id = outbound.asset_id
                WHERE inbound.tx_hash != outbound.tx_hash
                  AND outbound.block_timestamp_unix_ms >= inbound.block_timestamp_unix_ms
                  AND outbound.block_timestamp_unix_ms <= inbound.block_timestamp_unix_ms + 600000
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .bind(&self.network_id)
            .bind(address)
            .fetch_one::<PassThroughRow>()
            .await
            .context("failed to detect Ethereum rapid pass-through evidence")
    }

    #[allow(clippy::too_many_arguments)]
    async fn persist(
        &self,
        address: &str,
        assessment_id: &str,
        signals: &[RiskSignalOutput],
        risk_score: f64,
        risk_level: &str,
        top_reasons: &[String],
        evidence_refs: &[String],
        exposure_run_id: &str,
        as_of_block: u64,
        created_at: u64,
    ) -> anyhow::Result<()> {
        let rows = signals
            .iter()
            .map(|signal| RiskSignalRow {
                signal_id: stable_id("eth_risk_signal", &[assessment_id, &signal.signal_type]),
                assessment_id: assessment_id.to_string(),
                network_id: self.network_id.clone(),
                address: address.to_string(),
                policy_version: self.policy_version.clone(),
                signal_type: signal.signal_type.clone(),
                signal_strength: signal.signal_strength,
                contribution: signal.contribution,
                summary: signal.summary.clone(),
                evidence_refs: signal.evidence_refs.clone(),
                evidence_json: signal.evidence.to_string(),
                as_of_block,
                created_at_unix_ms: created_at,
            })
            .collect::<Vec<_>>();
        insert_rows(&self.client, "wallet_risk_signals", &rows).await?;
        insert_rows(
            &self.client,
            "wallet_risk_assessments",
            &[RiskAssessmentRow {
                assessment_id: assessment_id.to_string(),
                network_id: self.network_id.clone(),
                address: address.to_string(),
                policy_version: self.policy_version.clone(),
                risk_score,
                risk_level: risk_level.to_string(),
                signal_count: signals.len().try_into().unwrap_or(u16::MAX),
                top_reasons: top_reasons.to_vec(),
                evidence_refs: evidence_refs.to_vec(),
                exposure_run_id: exposure_run_id.to_string(),
                as_of_block,
                as_of_unix_ms: created_at,
                scoring_method: SCORING_METHOD.to_string(),
                created_at_unix_ms: created_at,
            }],
        )
        .await
    }
}

fn strongest_exposure<'a>(
    paths: &'a [ExposurePathRow],
    direction: &str,
    predicate: impl Fn(&ExposurePathRow) -> bool,
) -> Option<&'a ExposurePathRow> {
    paths
        .iter()
        .filter(|path| path.direction == direction && predicate(path))
        .max_by(|left, right| left.exposure_score.total_cmp(&right.exposure_score))
}

fn push_exposure_signal(
    signals: &mut Vec<RiskSignalOutput>,
    rules: &HashMap<String, RuleRow>,
    signal_type: &str,
    path: &ExposurePathRow,
) {
    push_signal(
        signals,
        rules,
        signal_type,
        path.exposure_score.clamp(0.0, 1.0),
        path.relationship_ids.clone(),
        serde_json::json!({
            "path_id": path.path_id,
            "seed_address": path.seed_address,
            "seed_entity_id": path.seed_entity_id,
            "hop_count": path.hop_count,
            "service_mediated": path.service_mediated == 1,
            "path_addresses": path.path_addresses,
            "tx_hashes": path.tx_hashes
        }),
    );
}

fn push_signal(
    signals: &mut Vec<RiskSignalOutput>,
    rules: &HashMap<String, RuleRow>,
    signal_type: &str,
    strength: f64,
    evidence_refs: Vec<String>,
    evidence: serde_json::Value,
) {
    let Some(rule) = rules.get(signal_type) else {
        return;
    };
    let strength = strength.clamp(0.0, 1.0);
    signals.push(RiskSignalOutput {
        signal_type: signal_type.to_string(),
        signal_strength: strength,
        contribution: rule.max_contribution * strength,
        summary: rule.description.clone(),
        evidence_refs,
        evidence,
    });
}

fn nearby_semantic_sequence(
    events: &[SemanticEvidenceRow],
    left: impl Fn(&SemanticEvidenceRow) -> bool,
    right: impl Fn(&SemanticEvidenceRow) -> bool,
    window_ms: u64,
) -> Option<Vec<String>> {
    for first in events.iter().filter(|event| left(event)) {
        for second in events.iter().filter(|event| right(event)) {
            if first
                .block_timestamp_unix_ms
                .abs_diff(second.block_timestamp_unix_ms)
                <= window_ms
            {
                return Some(vec![first.event_id.clone(), second.event_id.clone()]);
            }
        }
    }
    None
}

async fn insert_rows<R>(client: &Client, table: &str, rows: &[R]) -> anyhow::Result<()>
where
    R: RowOwned + RowWrite,
{
    if rows.is_empty() {
        return Ok(());
    }
    let mut insert = client
        .insert::<R>(table)
        .await
        .with_context(|| format!("failed to start insert into {table}"))?;
    for row in rows {
        insert.write(row).await?;
    }
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

fn unix_time_millis() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis()
        .try_into()
        .context("Unix timestamp does not fit UInt64")?)
}

#[cfg(test)]
mod tests {
    use super::{ExposurePathRow, strongest_exposure};

    #[test]
    fn selects_strongest_matching_exposure() {
        let paths = vec![
            ExposurePathRow {
                path_id: "a".into(),
                direction: "RECEIVED_FROM_SEED".into(),
                hop_count: 1,
                exposure_score: 0.2,
                seed_address: String::new(),
                seed_entity_id: String::new(),
                service_mediated: 0,
                relationship_ids: vec![],
                tx_hashes: vec![],
                path_addresses: vec![],
            },
            ExposurePathRow {
                path_id: "b".into(),
                direction: "RECEIVED_FROM_SEED".into(),
                hop_count: 1,
                exposure_score: 0.7,
                seed_address: String::new(),
                seed_entity_id: String::new(),
                service_mediated: 0,
                relationship_ids: vec![],
                tx_hashes: vec![],
                path_addresses: vec![],
            },
        ];
        assert_eq!(
            strongest_exposure(&paths, "RECEIVED_FROM_SEED", |path| path.hop_count == 1)
                .unwrap()
                .path_id,
            "b"
        );
    }
}
