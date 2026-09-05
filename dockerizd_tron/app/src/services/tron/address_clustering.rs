use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::models::tron::intelligence::AddressClusterClaimRow;
use crate::services::tron::entity_intelligence::submit_cluster_claims;

const CLUSTER_SOURCE: &str = "tron_structural_heuristics_v1";
const DEPOSIT_METHOD: &str = "TRON_EXCHANGE_DEPOSIT_SWEEP_V1";
const SERVICE_METHOD: &str = "TRON_SERVICE_ACTIVITY_V1";

#[derive(Debug, Clone, Serialize)]
pub struct ClusterDiscoveryReport {
    pub start_block: u64,
    pub end_block: u64,
    pub anchor_candidates_examined: usize,
    pub service_candidates_examined: usize,
    pub deposit_claims_proposed: usize,
    pub service_claims_proposed: usize,
    pub claims_written: usize,
    pub unchanged_claims_skipped: usize,
    pub candidates_below_threshold: usize,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct AnchorTransferCandidate {
    candidate_address: String,
    anchor_address: String,
    anchor_entity_id: String,
    anchor_exchange_name: String,
    anchor_confidence: f32,
    sweep_count: u64,
    first_seen_block: u64,
    last_seen_block: u64,
    evidence_tx_hashes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct AddressActivity {
    address: String,
    inbound_txs: u64,
    outbound_txs: u64,
    unique_senders: u64,
    unique_receivers: u64,
    first_seen_block: u64,
    last_seen_block: u64,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct ExistingClaim {
    claim_id: String,
    address: String,
    cluster_id: String,
    claim_method: String,
    created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct StoredAddress {
    address: String,
}

pub async fn discover_address_cluster_claims(
    clickhouse: &Client,
    start_block: u64,
    end_block: u64,
    max_claims: usize,
) -> Result<ClusterDiscoveryReport> {
    let max_claims = max_claims.clamp(1, 50_000);
    let query_limit = (max_claims as u64).saturating_mul(2).max(100);
    // اینجا exchange های شناخته شده را می گیرد
    let active_exchange_addresses = load_active_exchange_addresses(clickhouse).await?;
    let existing_claims = load_existing_claims(clickhouse).await?;
    let existing_ids = existing_claims
        .iter()
        .map(|claim| claim.claim_id.clone())
        .collect::<HashSet<_>>();
    let latest_claims = latest_claim_by_key(existing_claims);
    // دنبال wallet هایی می‌گردد که به Exchange های شناخته‌شده پول sweep کرده‌اند.
    let anchor_candidates =
        load_anchor_candidates(clickhouse, start_block, end_block, query_limit).await?;
    let candidate_addresses = anchor_candidates
        .iter()
        .map(|candidate| candidate.candidate_address.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let candidate_activity =
        load_activity_for_addresses(clickhouse, &candidate_addresses, start_block, end_block)
            .await?
            .into_iter()
            .map(|activity| (activity.address.clone(), activity))
            .collect::<HashMap<_, _>>();

    let mut proposed = Vec::<AddressClusterClaimRow>::new();
    let mut unchanged_claims_skipped = 0usize;
    let mut candidates_below_threshold = 0usize;
    for candidate in &anchor_candidates {
        if proposed.len() >= max_claims {
            break;
        }
        if active_exchange_addresses.contains(&candidate.candidate_address) {
            candidates_below_threshold += 1;
            continue;
        }
        let Some(activity) = candidate_activity.get(&candidate.candidate_address) else {
            candidates_below_threshold += 1;
            continue;
        };
        // این تابع بررسی میکند که آیا این ولت یه ولت deposit هست یا نه
        if !is_exchange_deposit_activity(activity, candidate.sweep_count) {
            candidates_below_threshold += 1;
            continue;
        }

        let cluster_id = format!(
            "cluster:tron:{}",
            identifier_component(&candidate.anchor_entity_id)
        );
        let confidence = deposit_confidence(activity, candidate);
        let evidence_json = json!({
            "method_version": 1,
            "anchor_exchange": candidate.anchor_exchange_name,
            "anchor_role": "APPROVED_SERVICE",
            "sweep_count": candidate.sweep_count,
            "inbound_txs": activity.inbound_txs,
            "outbound_txs": activity.outbound_txs,
            "unique_senders": activity.unique_senders,
            "unique_receivers": activity.unique_receivers,
            "first_seen_block": activity.first_seen_block,
            "last_seen_block": activity.last_seen_block,
        })
        .to_string();
        if !push_claim_if_changed(
            &mut proposed,
            &existing_ids,
            &latest_claims,
            ProposedClaim {
                address: candidate.candidate_address.clone(),
                cluster_id,
                cluster_type: "EXCHANGE_SERVICE".to_string(),
                address_role: "DEPOSIT".to_string(),
                claim_method: DEPOSIT_METHOD.to_string(),
                confidence,
                evidence_tx_hashes: candidate.evidence_tx_hashes.clone(),
                evidence_addresses: vec![candidate.anchor_address.clone()],
                evidence_json,
                source_record_id: format!(
                    "blocks:{}-{}:{}",
                    candidate.first_seen_block, candidate.last_seen_block, candidate.anchor_address
                ),
            },
        ) {
            unchanged_claims_skipped += 1;
        }
    }
    let deposit_claims_proposed = proposed.len();
    let mut service_candidates_examined = 0usize;

    if proposed.len() < max_claims {
        let remaining = max_claims - proposed.len();
        let service_candidates = load_service_candidates(
            clickhouse,
            start_block,
            end_block,
            (remaining as u64).saturating_mul(2).max(100),
        )
        .await?;
        service_candidates_examined = service_candidates.len();
        for activity in &service_candidates {
            if proposed.len() >= max_claims {
                break;
            }
            if active_exchange_addresses.contains(&activity.address) {
                candidates_below_threshold += 1;
                continue;
            }
            // تشخیص HOT / SWEEP / WITHDRAW
            let Some((role, confidence)) = classify_service_activity(activity) else {
                candidates_below_threshold += 1;
                continue;
            };
            let evidence_json = json!({
                "method_version": 1,
                "inbound_txs": activity.inbound_txs,
                "outbound_txs": activity.outbound_txs,
                "unique_senders": activity.unique_senders,
                "unique_receivers": activity.unique_receivers,
                "first_seen_block": activity.first_seen_block,
                "last_seen_block": activity.last_seen_block,
            })
            .to_string();
            if !push_claim_if_changed(
                &mut proposed,
                &existing_ids,
                &latest_claims,
                ProposedClaim {
                    address: activity.address.clone(),
                    cluster_id: format!(
                        "cluster:tron:unattributed_service:{}",
                        activity.address.to_ascii_lowercase()
                    ),
                    cluster_type: "UNATTRIBUTED_SERVICE".to_string(),
                    address_role: role.to_string(),
                    claim_method: SERVICE_METHOD.to_string(),
                    confidence,
                    evidence_tx_hashes: Vec::new(),
                    evidence_addresses: Vec::new(),
                    evidence_json,
                    source_record_id: format!(
                        "blocks:{}-{}",
                        activity.first_seen_block, activity.last_seen_block
                    ),
                },
            ) {
                unchanged_claims_skipped += 1;
            }
        }
    }

    let service_claims_proposed = proposed.len().saturating_sub(deposit_claims_proposed);
    let claims_written = submit_cluster_claims(clickhouse, proposed).await?;

    Ok(ClusterDiscoveryReport {
        start_block,
        end_block,
        anchor_candidates_examined: anchor_candidates.len(),
        service_candidates_examined,
        deposit_claims_proposed,
        service_claims_proposed,
        claims_written,
        unchanged_claims_skipped,
        candidates_below_threshold,
    })
}

struct ProposedClaim {
    address: String,
    cluster_id: String,
    cluster_type: String,
    address_role: String,
    claim_method: String,
    confidence: f32,
    evidence_tx_hashes: Vec<String>,
    evidence_addresses: Vec<String>,
    evidence_json: String,
    source_record_id: String,
}

fn push_claim_if_changed(
    claims: &mut Vec<AddressClusterClaimRow>,
    existing_ids: &HashSet<String>,
    latest_claims: &HashMap<String, ExistingClaim>,
    mut proposed: ProposedClaim,
) -> bool {
    proposed.evidence_tx_hashes.sort();
    proposed.evidence_tx_hashes.dedup();
    proposed.evidence_addresses.sort();
    proposed.evidence_addresses.dedup();
    let evidence_identity = format!(
        "{}|{}|{}",
        proposed.evidence_tx_hashes.join(","),
        proposed.evidence_addresses.join(","),
        proposed.evidence_json
    );
    let claim_id = content_id(
        "tron_cluster_claim",
        &[
            &proposed.address,
            &proposed.cluster_id,
            &proposed.claim_method,
            &evidence_identity,
        ],
    );
    if existing_ids.contains(&claim_id) {
        return false;
    }
    let key = claim_key(
        &proposed.address,
        &proposed.cluster_id,
        &proposed.claim_method,
    );
    let supersedes_claim_id = latest_claims
        .get(&key)
        .map(|claim| claim.claim_id.clone())
        .unwrap_or_default();
    claims.push(AddressClusterClaimRow {
        claim_id,
        chain: "tron".to_string(),
        address: proposed.address,
        cluster_id: proposed.cluster_id,
        cluster_type: proposed.cluster_type,
        address_role: proposed.address_role,
        claim_method: proposed.claim_method,
        confidence: proposed.confidence,
        source: CLUSTER_SOURCE.to_string(),
        source_record_id: proposed.source_record_id,
        evidence_tx_hashes: proposed.evidence_tx_hashes,
        evidence_addresses: proposed.evidence_addresses,
        evidence_json: proposed.evidence_json,
        supersedes_claim_id,
        review_status: "PENDING".to_string(),
        created_by: "tron_cluster_worker".to_string(),
        created_at_unix_ms: now_unix_ms(),
    });
    true
}

async fn load_active_exchange_addresses(clickhouse: &Client) -> Result<HashSet<String>> {
    Ok(clickhouse
        .query(
            r#"
            SELECT address
            FROM exchange_addresses FINAL
            WHERE is_active = 1
            "#,
        )
        .fetch_all::<StoredAddress>()
        .await?
        .into_iter()
        .map(|row| row.address)
        .collect())
}

async fn load_existing_claims(clickhouse: &Client) -> Result<Vec<ExistingClaim>> {
    clickhouse
        .query(
            r#"
            SELECT claim_id, address, cluster_id, claim_method, created_at_unix_ms
            FROM address_cluster_claims FINAL
            WHERE chain = 'tron' AND source = ?
            "#,
        )
        .bind(CLUSTER_SOURCE)
        .fetch_all::<ExistingClaim>()
        .await
        .map_err(Into::into)
}

async fn load_anchor_candidates(
    clickhouse: &Client,
    start_block: u64,
    end_block: u64,
    limit: u64,
) -> Result<Vec<AnchorTransferCandidate>> {
    clickhouse
        .query(
            r#"
            SELECT
                relationship.from_address AS candidate_address,
                relationship.to_address AS anchor_address,
                anchor.entity_id AS anchor_entity_id,
                anchor.exchange_name AS anchor_exchange_name,
                anchor.confidence AS anchor_confidence,
                count() AS sweep_count,
                min(relationship.block_number) AS first_seen_block,
                max(relationship.block_number) AS last_seen_block,
                groupUniqArray(20)(relationship.tx_hash) AS evidence_tx_hashes
            FROM address_relationships_canonical AS relationship
            INNER JOIN exchange_addresses AS anchor FINAL
                ON anchor.address = relationship.to_address
            WHERE relationship.block_number BETWEEN ? AND ?
              AND relationship.from_address != ''
              AND relationship.from_address != relationship.to_address
              AND anchor.is_active = 1
              AND anchor.confidence >= 0.80
              AND anchor.address_role IN ('HOT', 'SWEEP', 'TREASURY', 'INTERNAL')
            GROUP BY
                relationship.from_address,
                relationship.to_address,
                anchor.entity_id,
                anchor.exchange_name,
                anchor.confidence
            ORDER BY sweep_count DESC, last_seen_block DESC
            LIMIT ?
            "#,
        )
        .bind(start_block)
        .bind(end_block)
        .bind(limit)
        .fetch_all::<AnchorTransferCandidate>()
        .await
        .map_err(Into::into)
}

async fn load_activity_for_addresses(
    clickhouse: &Client,
    addresses: &[String],
    start_block: u64,
    end_block: u64,
) -> Result<Vec<AddressActivity>> {
    if addresses.is_empty() {
        return Ok(Vec::new());
    }
    clickhouse
        .query(
            r#"
            SELECT
                address,
                countIf(to_address = address) AS inbound_txs,
                countIf(from_address = address) AS outbound_txs,
                uniqExactIf(from_address, to_address = address) AS unique_senders,
                uniqExactIf(to_address, from_address = address) AS unique_receivers,
                min(block_number) AS first_seen_block,
                max(block_number) AS last_seen_block
            FROM address_relationships_canonical
            ARRAY JOIN [from_address, to_address] AS address
            WHERE block_number BETWEEN ? AND ?
              AND address IN ?
            GROUP BY address
            "#,
        )
        .bind(start_block)
        .bind(end_block)
        .bind(addresses)
        .fetch_all::<AddressActivity>()
        .await
        .map_err(Into::into)
}

async fn load_service_candidates(
    clickhouse: &Client,
    start_block: u64,
    end_block: u64,
    limit: u64,
) -> Result<Vec<AddressActivity>> {
    clickhouse
        .query(
            r#"
            SELECT
                address,
                countIf(to_address = address) AS inbound_txs,
                countIf(from_address = address) AS outbound_txs,
                uniqExactIf(from_address, to_address = address) AS unique_senders,
                uniqExactIf(to_address, from_address = address) AS unique_receivers,
                min(block_number) AS first_seen_block,
                max(block_number) AS last_seen_block
            FROM address_relationships_canonical
            ARRAY JOIN [from_address, to_address] AS address
            WHERE block_number BETWEEN ? AND ?
              AND address != ''
            GROUP BY address
            HAVING
                (inbound_txs >= 25 AND unique_senders >= 20)
                OR (outbound_txs >= 25 AND unique_receivers >= 20)
            ORDER BY greatest(inbound_txs, outbound_txs) DESC
            LIMIT ?
            "#,
        )
        .bind(start_block)
        .bind(end_block)
        .bind(limit)
        .fetch_all::<AddressActivity>()
        .await
        .map_err(Into::into)
}

fn is_exchange_deposit_activity(activity: &AddressActivity, sweep_count: u64) -> bool {
    activity.inbound_txs >= 2
        && activity.unique_senders >= 2
        && activity.outbound_txs >= 1
        && activity.unique_receivers <= 3
        && sweep_count >= 1
}

// تشخیص اینکه HOT . Sweep. Widthraw ولت هست کدومشه
fn classify_service_activity(activity: &AddressActivity) -> Option<(&'static str, f32)> {
    if activity.inbound_txs >= 100
        && activity.outbound_txs >= 100
        && activity.unique_senders >= 50
        && activity.unique_receivers >= 50
    {
        return Some(("HOT", 0.80));
    }
    if activity.inbound_txs >= 25
        && activity.unique_senders >= 20
        && activity.outbound_txs > 0
        && activity.unique_receivers <= 10
    {
        return Some(("SWEEP", 0.72));
    }
    if activity.outbound_txs >= 25
        && activity.unique_receivers >= 20
        && activity.unique_senders <= 10
    {
        return Some(("WITHDRAW", 0.68));
    }
    None
}

fn deposit_confidence(activity: &AddressActivity, candidate: &AnchorTransferCandidate) -> f32 {
    let sender_evidence = (activity.unique_senders.min(10) as f32) / 10.0;
    let sweep_evidence = (candidate.sweep_count.min(5) as f32) / 5.0;
    (0.55 + 0.15 * sender_evidence + 0.15 * sweep_evidence)
        .min(0.90)
        .min(candidate.anchor_confidence)
}

fn latest_claim_by_key(claims: Vec<ExistingClaim>) -> HashMap<String, ExistingClaim> {
    let mut latest = HashMap::new();
    for claim in claims {
        let key = claim_key(&claim.address, &claim.cluster_id, &claim.claim_method);
        let replace = latest.get(&key).is_none_or(|existing: &ExistingClaim| {
            claim.created_at_unix_ms > existing.created_at_unix_ms
        });
        if replace {
            latest.insert(key, claim);
        }
    }
    latest
}

fn claim_key(address: &str, cluster_id: &str, method: &str) -> String {
    format!("{address}|{cluster_id}|{method}")
}

fn content_id(prefix: &str, parts: &[&str]) -> String {
    let digest = format!("{:x}", Sha256::digest(parts.join("|").as_bytes()));
    format!("{prefix}_{}", &digest[..24])
}

fn identifier_component(value: &str) -> String {
    let mut output = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if matches!(ch, ':' | '-' | '_') {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    output
}

fn now_unix_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(inbound: u64, outbound: u64, senders: u64, receivers: u64) -> AddressActivity {
        AddressActivity {
            address: "TTest".to_string(),
            inbound_txs: inbound,
            outbound_txs: outbound,
            unique_senders: senders,
            unique_receivers: receivers,
            first_seen_block: 1,
            last_seen_block: 10,
        }
    }

    #[test]
    fn deposit_claim_requires_multiple_funders_and_narrow_sweep() {
        assert!(is_exchange_deposit_activity(&activity(3, 1, 3, 1), 1));
        assert!(!is_exchange_deposit_activity(&activity(1, 1, 1, 1), 1));
        assert!(!is_exchange_deposit_activity(&activity(3, 5, 3, 5), 1));
    }

    #[test]
    fn service_roles_are_structural_not_entity_names() {
        assert_eq!(
            classify_service_activity(&activity(100, 100, 60, 60)),
            Some(("HOT", 0.80))
        );
        assert_eq!(
            classify_service_activity(&activity(100, 3, 60, 3)),
            Some(("SWEEP", 0.72))
        );
        assert_eq!(
            classify_service_activity(&activity(3, 100, 3, 60)),
            Some(("WITHDRAW", 0.68))
        );
    }

    #[test]
    fn claim_identity_is_replay_safe() {
        let first = content_id("claim", &["address", "cluster", "method", "evidence"]);
        let replay = content_id("claim", &["address", "cluster", "method", "evidence"]);
        let changed = content_id("claim", &["address", "cluster", "method", "new"]);

        assert_eq!(first, replay);
        assert_ne!(first, changed);
    }
}
