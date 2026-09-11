use crate::investigation::graph::{continues, edges};
use crate::{
    BSC_NETWORK_ID,
    db::warehouse::Warehouse,
    domain::normalize_evm_address,
    investigation::{Edge, SearchOptions, ZERO_ADDRESS, now_ms},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub network_id: String,
    pub claim_id: String,
    pub address: String,
    pub claim_kind: String,
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f64,
    pub risk_level: u8,
    pub is_exposure_seed: bool,
    pub seed_category: String,
    pub source_id: String,
    pub source_reference: String,
    pub evidence_refs: Vec<String>,
    pub review_status: String,
    pub reviewed_by: String,
    pub review_note: String,
}
impl Claim {
    pub fn validate(&mut self) -> Result<()> {
        ensure!(
            self.network_id == BSC_NETWORK_ID,
            "claim belongs to another network"
        );
        self.address = normalize_evm_address(&self.address)?;
        ensure!(
            self.address != ZERO_ADDRESS,
            "zero address cannot be attributed"
        );
        ensure!(
            matches!(self.claim_kind.as_str(), "label" | "cluster"),
            "invalid claim kind"
        );
        ensure!(
            matches!(
                self.review_status.as_str(),
                "pending" | "approved" | "rejected"
            ),
            "invalid review status"
        );
        ensure!(
            self.confidence.is_finite()
                && (0.0..=1.0).contains(&self.confidence)
                && self.risk_level <= 100,
            "invalid confidence/risk"
        );
        for value in [
            &self.claim_id,
            &self.entity_id,
            &self.entity_name,
            &self.entity_type,
            &self.source_id,
            &self.source_reference,
        ] {
            ensure!(
                !value.trim().is_empty()
                    && value.len() <= 1000
                    && !value.chars().any(char::is_control),
                "invalid identity/provenance field"
            );
        }
        ensure!(
            !self.evidence_refs.is_empty()
                && self.evidence_refs.len() <= 50
                && self
                    .evidence_refs
                    .iter()
                    .all(|v| !v.trim().is_empty() && v.len() <= 2000),
            "evidence references are required"
        );
        ensure!(
            self.review_status == "pending" || !self.reviewed_by.trim().is_empty(),
            "review decision requires reviewer"
        );
        ensure!(
            !self.is_exposure_seed
                || (self.claim_kind == "label"
                    && self.risk_level > 0
                    && !self.seed_category.trim().is_empty()),
            "only explicit risky label claims can be exposure seeds"
        );
        Ok(())
    }
}

pub async fn import(db: &Warehouse, mut claims: Vec<Claim>, dry_run: bool) -> Result<usize> {
    ensure!(
        !claims.is_empty() && claims.len() <= 10000,
        "import must contain 1..10000 claims"
    );
    let mut ids = HashSet::new();
    for claim in &mut claims {
        claim.validate()?;
        ensure!(
            ids.insert(claim.claim_id.clone()),
            "duplicate claim id in batch"
        );
    }
    if dry_run {
        return Ok(claims.len());
    }
    let max = db
        .rows(
            "SELECT max(revision) AS revision FROM intelligence_claims",
            &[],
        )
        .await?;
    let base = now_ms().max(max[0]["revision"].as_u64().unwrap_or(0) + 1);
    let mut rows = Vec::new();
    for (index, claim) in claims.iter().enumerate() {
        let mut row = serde_json::to_value(claim)?;
        row["revision"] = json!(base + index as u64);
        row["created_at_unix_ms"] = json!(now_ms());
        rows.push(row);
    }
    db.insert("intelligence_claims", &rows).await?;
    Ok(rows.len())
}

pub async fn active(db: &Warehouse, address: &str) -> Result<Vec<Value>> {
    db.rows("SELECT claim_id,claim_kind,address,entity_id,entity_name,entity_type,address_role,confidence,
        risk_level,is_exposure_seed,seed_category,source_id,source_reference,evidence_refs,review_status,
        reviewed_by,revision,created_at_unix_ms,entity_id AS cluster_id,'reviewed_attribution' AS membership_type
        FROM intelligence_active WHERE network_id='eip155:56' AND address={address:String}
        ORDER BY confidence DESC,claim_id LIMIT 100",&[("address",address)]).await
}

pub async fn candidates(db: &Warehouse, address: &str) -> Result<Vec<Value>> {
    // Shared destinations are leads for review, never automatic common ownership.
    db.rows(
        "SELECT to_address AS destination, asset_id, uniqExact(from_address) AS distinct_senders,
        count() AS transfers, groupUniqArray(20)(relationship_id) AS evidence_refs,
        'shared_destination_lead' AS heuristic, 'pending' AS review_status
        FROM address_relationships_canonical
        WHERE to_address IN (SELECT to_address FROM address_relationships_canonical
            WHERE from_address={address:String} LIMIT 100)
        AND to_address!='0x0000000000000000000000000000000000000000'
        AND from_address!=to_address AND from_address!='0x0000000000000000000000000000000000000000'
        GROUP BY to_address,asset_id HAVING distinct_senders BETWEEN 3 AND 20
        ORDER BY transfers DESC LIMIT 25",
        &[("address", address)],
    )
    .await
}

fn service(labels: &[Value]) -> bool {
    labels.iter().any(|v| {
        matches!(
            v["entity_type"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "exchange" | "bridge" | "custodian" | "dex"
        )
    })
}

pub async fn exposure(
    db: &Warehouse,
    address: &str,
    height: u64,
    at: u64,
) -> Result<(Vec<Value>, Value)> {
    let mut labels = HashMap::new();
    labels.insert(address.to_owned(), active(db, address).await?);
    let subject_service = service(&labels[address]);
    let mut cache = HashMap::<String, Vec<Edge>>::new();
    let mut queue = VecDeque::from([
        (vec![address.to_owned()], Vec::<Edge>::new(), false),
        (vec![address.to_owned()], Vec::<Edge>::new(), true),
    ]);
    let mut results = Vec::new();
    let mut states = 0;
    let mut truncated = false;
    while let Some((addresses, path, incoming)) = queue.pop_front() {
        states += 1;
        if states > 1000 || cache.len() >= 32 || results.len() >= 50 {
            truncated = true;
            break;
        }
        let current = addresses.last().unwrap();
        if !cache.contains_key(current) {
            let mut rows = edges(db, current, &SearchOptions::default(), height, 101).await?;
            if rows.len() > 100 {
                truncated = true;
                rows.truncate(100);
            }
            cache.insert(current.clone(), rows);
        }
        for edge in &cache[current] {
            let next = if incoming && edge.to_address == *current {
                &edge.from_address
            } else if !incoming && edge.from_address == *current {
                &edge.to_address
            } else {
                continue;
            };
            if next == ZERO_ADDRESS
                || addresses.contains(next)
                || path
                    .last()
                    .is_some_and(|last| !continues(last, edge, incoming))
            {
                continue;
            }
            let mut next_addresses = addresses.clone();
            next_addresses.push(next.clone());
            let mut next_path = path.clone();
            next_path.push(edge.clone());
            if !labels.contains_key(next) {
                if labels.len() >= 128 {
                    truncated = true;
                    continue;
                }
                labels.insert(next.clone(), active(db, next).await?);
            }
            let boundary = subject_service || service(&labels[next]);
            for seed in labels[next]
                .iter()
                .filter(|v| v["is_exposure_seed"] == true)
            {
                let age =
                    at.saturating_sub(next_path[0].block_timestamp_unix_ms) as f64 / 86_400_000.0;
                let amounts = next_path
                    .iter()
                    .map(|e| e.amount.parse::<f64>().unwrap_or(0.0))
                    .collect::<Vec<_>>();
                let max = amounts.iter().copied().fold(0.0_f64, f64::max);
                let min = amounts.iter().copied().fold(f64::MAX, f64::min);
                let amount_continuity = if max > 0.0 { min / max } else { 0.0 };
                let strength = seed["confidence"].as_f64().unwrap_or(0.0)
                    * seed["risk_level"].as_f64().unwrap_or(0.0)
                    / 100.0
                    * 0.65_f64.powi(next_path.len() as i32 - 1)
                    * 0.5_f64.powf(age / 365.0)
                    * amount_continuity;
                let ids = next_path.iter().map(|e| &e.id).collect::<Vec<_>>();
                let hash = format!(
                    "{:x}",
                    Sha256::digest(format!("{address}:{incoming}:{ids:?}"))
                );
                results.push(json!({"path_id":hash,"run_id":format!("bsc-exposure-v1:{at}:{height}"),"seed_address":next,
                    "seed_entity_id":seed["entity_id"],"seed_category":seed["seed_category"],"seed_claim":seed,
                    "direction":if incoming {"RECEIVED_FROM_SEED"} else {"SENT_TO_SEED"},
                    "hop_count":next_path.len(),"asset_id":edge.asset_id,"exposure_score":strength.clamp(0.0,1.0),
                    "service_mediated":boundary,"continuity_type":"same_asset_ordered_transactions",
                    "path_addresses":next_addresses,"relationship_ids":ids,"tx_hashes":next_path.iter().map(|e|&e.tx_hash).collect::<Vec<_>>(),
                    "transfers":next_path,"amount_continuity_ratio":amount_continuity,
                    "policy_version":"bsc_exposure_v1","as_of_block":height}));
            }
            if !boundary && next_path.len() < 3 {
                if queue.len() < 1000 {
                    queue.push_back((next_addresses, next_path, incoming));
                } else {
                    truncated = true;
                }
            }
        }
    }
    results.sort_by(|a, b| {
        b["exposure_score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["exposure_score"].as_f64().unwrap_or(0.0))
    });
    truncated |= results.len() > 50;
    results.truncate(50);
    Ok((
        results,
        json!({"max_hops":3,"max_addresses":32,"per_address_limit":100,"truncated":truncated,
        "policy_version":"bsc_exposure_v1","storage":"central_investigation_snapshot","source":"canonical_clickhouse",
        "limitations":["Service boundaries stop traversal. No common-exchange guilt association.",
            "Amount continuity is a same-asset size heuristic, not attributed illicit fund value.",
            "No paths found does not prove absence of exposure. Limits and missing labels apply."]}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_claims_need_provenance_and_cannot_promote_clusters_to_seeds() {
        let mut c:Claim=serde_json::from_value(json!({"network_id":BSC_NETWORK_ID,"claim_id":"x",
            "address":"0x1111111111111111111111111111111111111111","claim_kind":"label","entity_id":"x","entity_name":"Example",
            "entity_type":"scam","address_role":"wallet","confidence":1.0,"risk_level":90,"is_exposure_seed":true,
            "seed_category":"scam","source_id":"case","source_reference":"case:1","evidence_refs":["tx:1"],
            "review_status":"approved","reviewed_by":"","review_note":""})).unwrap();
        assert!(c.validate().is_err());
        c.reviewed_by = "analyst".into();
        assert!(c.validate().is_ok());
        c.claim_kind = "cluster".into();
        assert!(c.validate().is_err());
        c.is_exposure_seed = false;
        c.network_id = "eip155:1".into();
        assert!(c.validate().is_err());
    }
}
