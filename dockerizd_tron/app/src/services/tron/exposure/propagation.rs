//! Bounded observed directed paths. No allocation or fund-continuity assertion.
use crate::{
    db::{Client, now_ms},
    models::tron::exposure::{AddressExposureRow, ExposurePathEvidence},
};
use anyhow::{Result, ensure};
use num_bigint::BigUint;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
const MAX_EDGES_PER_ADDRESS: usize = 2000;
#[derive(Deserialize, clickhouse::Row)]
struct Edge {
    relationship_id: String,
    to_address: String,
    asset_id: String,
    tx_hash: String,
    block_number: u64,
    transaction_index: u32,
    timestamp: u64,
    amount: String,
    service_mediated: u8,
}
struct Step {
    address: String,
    score: f64,
    hops: u8,
    position: Option<(u64, u32)>,
    path: ExposurePathEvidence,
}
pub async fn propagate_exposure(
    client: &Client,
    seed: &str,
    max_hops: u8,
    run: &str,
) -> Result<Vec<AddressExposureRow>> {
    let max_hops = max_hops.clamp(1, 10);
    let now = now_ms();
    let mut queue = VecDeque::from([Step {
        address: seed.into(),
        score: 1.0,
        hops: 0,
        position: None,
        path: ExposurePathEvidence {
            subjects: vec![seed.into()],
            ..Default::default()
        },
    }]);
    let mut best = HashMap::<(String, String, u64, u32), f64>::new();
    let mut aggregates = HashMap::<String, AddressExposureRow>::new();
    let mut expanded = 0;
    while let Some(step) = queue.pop_front() {
        if step.hops >= max_hops {
            continue;
        }
        expanded += 1;
        ensure!(
            expanded <= 50000,
            "exposure expansion limit; partial run not published"
        );
        let edges = load_edges(client, &step.address).await?;
        let eligible = edges
            .iter()
            .filter(|e| {
                step.position
                    .is_none_or(|p| (e.block_number, e.transaction_index) > p)
                    && (step.path.asset_id.is_empty() || e.asset_id == step.path.asset_id)
            })
            .collect::<Vec<_>>();
        let mut totals = HashMap::<&str, BigUint>::new();
        for e in &eligible {
            *totals.entry(&e.asset_id).or_default() += e.amount.parse::<BigUint>()?;
        }
        for e in eligible {
            if e.to_address.is_empty() || step.path.subjects.contains(&e.to_address) {
                continue;
            }
            let total = totals[&e.asset_id.as_str()].to_string().parse::<f64>()?;
            let share = if total > 0.0 {
                (e.amount.parse::<f64>()? / total).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let weight = time_weight(now, e.timestamp);
            let score = super::scorer::edge_exposure_score(
                step.score,
                share,
                weight,
                e.service_mediated == 1,
            );
            let mut path = step.path.clone();
            path.subjects.push(e.to_address.clone());
            path.relationship_ids.push(e.relationship_id.clone());
            path.tx_hashes.push(e.tx_hash.clone());
            path.asset_id = e.asset_id.clone();
            path.amount_raw = e.amount.clone();
            if path.first_transfer_unix_ms == 0 {
                path.first_transfer_unix_ms = e.timestamp;
            }
            path.last_transfer_unix_ms = e.timestamp;
            let candidate = AddressExposureRow {
                source_address: seed.into(),
                exposed_address: e.to_address.clone(),
                hop_distance: step.hops + 1,
                exposure_score: score,
                path_count: 0,
                last_tx_hash: e.tx_hash.clone(),
                last_seen_block: e.block_number,
                exposure_type: if e.service_mediated == 1 {
                    "SERVICE_MEDIATED"
                } else {
                    "RECEIVED_FROM_SEED"
                }
                .into(),
                best_path_amount_share: share,
                best_path_time_weight: weight,
                service_mediated: e.service_mediated,
                propagation_run_id: run.into(),
                best_path: path.clone(),
            };
            let row = aggregates
                .entry(e.to_address.clone())
                .or_insert_with(|| candidate.clone());
            let count = row.path_count.saturating_add(1);
            if score > row.exposure_score {
                *row = candidate;
            }
            row.path_count = count;
            let key = (
                e.to_address.clone(),
                e.asset_id.clone(),
                e.block_number,
                e.transaction_index,
            );
            if may_expand(e.service_mediated) && score > best.get(&key).copied().unwrap_or(0.0) {
                ensure!(
                    best.len() < 100000 || best.contains_key(&key),
                    "exposure state limit; partial run not published"
                );
                best.insert(key, score);
                queue.push_back(Step {
                    address: e.to_address.clone(),
                    score,
                    hops: step.hops + 1,
                    position: Some((e.block_number, e.transaction_index)),
                    path,
                });
            }
        }
    }
    let mut rows = aggregates.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.exposure_score
            .total_cmp(&a.exposure_score)
            .then(a.exposed_address.cmp(&b.exposed_address))
    });
    Ok(rows)
}
async fn load_edges(client: &Client, address: &str) -> Result<Vec<Edge>> {
    let rows=client.query("SELECT r.relationship_id AS relationship_id,ifNull(r.to_address,'') AS to_address,r.asset_id AS asset_id,r.tx_hash AS tx_hash,r.block_number AS block_number,r.transaction_index AS transaction_index,r.block_timestamp_unix_ms AS timestamp,toString(r.amount_raw) AS amount,toUInt8(ifNull(r.to_address,'') IN (SELECT address FROM exchange_addresses WHERE is_active=1)) AS service_mediated FROM aml_address_relationships_canonical AS r WHERE r.network_id='tron:mainnet' AND r.from_address=? AND r.transfer_type IN ('native_transfer','trc10_transfer','trc20_transfer') AND r.amount_raw>0 ORDER BY r.block_number,r.transaction_index,r.event_index,r.event_sub_index LIMIT ?").bind(address).bind((MAX_EDGES_PER_ADDRESS+1) as u64).fetch_all::<Edge>().await?;
    ensure!(
        rows.len() <= MAX_EDGES_PER_ADDRESS,
        "exposure edge limit; partial run not published"
    );
    Ok(rows)
}
fn may_expand(service_mediated: u8) -> bool {
    service_mediated == 0
}
fn time_weight(now: u64, event: u64) -> f64 {
    0.5_f64
        .powf(now.saturating_sub(event) as f64 / (180.0 * 24.0 * 60.0 * 60.0 * 1000.0))
        .clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{may_expand, time_weight};

    #[test]
    fn service_wallets_are_terminal_boundaries() {
        assert!(may_expand(0));
        assert!(!may_expand(1));
    }

    #[test]
    fn time_weight_halves_after_180_days() {
        let half_life = 180 * 24 * 60 * 60 * 1_000;

        assert!((time_weight(half_life, 0) - 0.5).abs() < 0.000_001);
    }
}
