use std::collections::{HashMap, VecDeque};

use anyhow::{Result, ensure};
use chrono::Utc;
use clickhouse::Client;
use serde::Deserialize;

use crate::models::tron::exposure::AddressExposureRow;
use crate::services::tron::exposure::scorer::edge_exposure_score;

const MAX_EXPANDED_ADDRESSES: usize = 50_000;
const MAX_EDGES_PER_ADDRESS: u64 = 2_000;

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ExposureEdgeRow {
    to_address: String,
    token_address: String,
    tx_hash: String,
    block_number: u64,
    timestamp: u64,
    amount_share: f64,
    service_mediated: u8,
}

#[derive(Debug)]
struct ExposureAggregate {
    hop_distance: u8,
    exposure_score: f64,
    path_count: u32,
    last_tx_hash: String,
    last_seen_block: u64,
    amount_share: f64,
    time_weight: f64,
    service_mediated: u8,
}

pub async fn propagate_exposure(
    clickhouse: &Client,
    seed_address: &str,
    max_hops: u8,
    propagation_run_id: &str,
) -> Result<Vec<AddressExposureRow>> {
    let max_hops = max_hops.clamp(1, 10);
    let now_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let mut best_scores = HashMap::<(String, String, u64), f64>::new();
    let mut queue = VecDeque::<(String, f64, u8, Option<(String, u64)>)>::from([(
        seed_address.to_string(),
        1.0,
        0,
        None,
    )]);
    let mut aggregates = HashMap::<String, ExposureAggregate>::new();
    let mut expanded = 0usize;

    while let Some((current, current_score, hops, arrival)) = queue.pop_front() {
        if hops >= max_hops {
            continue;
        }
        ensure!(
            expanded < MAX_EXPANDED_ADDRESSES,
            "exposure expansion limit reached; refusing to publish a partial run as complete"
        );
        expanded += 1;

        for edge in load_outgoing_edges(clickhouse, &current, arrival.as_ref()).await? {
            if edge.to_address.is_empty()
                || edge.to_address == current
                || edge.to_address == seed_address
            {
                continue;
            }

            let next_hop = hops + 1;
            let time_weight = time_weight(now_unix_ms, edge.timestamp);
            let next_score = edge_exposure_score(
                current_score,
                edge.amount_share,
                time_weight,
                edge.service_mediated == 1,
            );
            let aggregate =
                aggregates
                    .entry(edge.to_address.clone())
                    .or_insert(ExposureAggregate {
                        hop_distance: next_hop,
                        exposure_score: next_score,
                        path_count: 0,
                        last_tx_hash: edge.tx_hash.clone(),
                        last_seen_block: edge.block_number,
                        amount_share: edge.amount_share,
                        time_weight,
                        service_mediated: edge.service_mediated,
                    });

            aggregate.path_count = aggregate.path_count.saturating_add(1);

            if next_score > aggregate.exposure_score {
                aggregate.hop_distance = next_hop;
                aggregate.exposure_score = next_score;
                aggregate.last_tx_hash = edge.tx_hash.clone();
                aggregate.last_seen_block = edge.block_number;
                aggregate.amount_share = edge.amount_share;
                aggregate.time_weight = time_weight;
                aggregate.service_mediated = edge.service_mediated;
            }

            let key = (
                edge.to_address.clone(),
                edge.token_address.clone(),
                edge.block_number,
            );
            let previous_best = best_scores.get(&key).copied().unwrap_or_default();
            // A service's pooled outgoing funds cannot be attributed to this seed.
            if may_expand(edge.service_mediated) && next_score > previous_best {
                ensure!(
                    best_scores.len() < 100_000 || best_scores.contains_key(&key),
                    "exposure state limit reached; refusing partial publication"
                );
                best_scores.insert(key, next_score);
                queue.push_back((
                    edge.to_address,
                    next_score,
                    next_hop,
                    Some((edge.token_address, edge.block_number)),
                ));
            }
        }
    }

    let mut rows = aggregates
        .into_iter()
        .map(|(exposed_address, aggregate)| AddressExposureRow {
            source_address: seed_address.to_string(),
            exposed_address,
            hop_distance: aggregate.hop_distance,
            exposure_score: aggregate.exposure_score,
            path_count: aggregate.path_count,
            last_tx_hash: aggregate.last_tx_hash,
            last_seen_block: aggregate.last_seen_block,
            exposure_type: if aggregate.service_mediated == 1 {
                "SERVICE_MEDIATED".to_string()
            } else {
                "DIRECTED_FUND_FLOW".to_string()
            },
            best_path_amount_share: aggregate.amount_share,
            best_path_time_weight: aggregate.time_weight,
            service_mediated: aggregate.service_mediated,
            propagation_run_id: propagation_run_id.to_string(),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        right
            .exposure_score
            .partial_cmp(&left.exposure_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.hop_distance.cmp(&right.hop_distance))
            .then_with(|| left.exposed_address.cmp(&right.exposed_address))
    });

    Ok(rows)
}

fn may_expand(service_mediated: u8) -> bool {
    service_mediated == 0
}

async fn load_outgoing_edges(
    clickhouse: &Client,
    address: &str,
    arrival: Option<&(String, u64)>,
) -> Result<Vec<ExposureEdgeRow>> {
    let (has_arrival, asset, after_block) = match arrival {
        Some((asset, block)) => (1u8, asset.as_str(), *block),
        None => (0, "", 0),
    };
    let mut rows = clickhouse
        .query(
            r#"
            WITH token_totals AS
            (
                SELECT
                    token_address,
                    sum(toFloat64(amount)) AS total_amount
                FROM address_relationships_canonical
                WHERE from_address = ?
                GROUP BY token_address
            ),
            exchange_wallets AS
            (
                SELECT address
                FROM exchange_addresses FINAL
                WHERE is_active = 1
            )
            SELECT
                ar.to_address AS to_address,
                ar.token_address AS token_address,
                ar.tx_hash AS tx_hash,
                ar.block_number AS block_number,
                ar.timestamp AS timestamp,
                if(
                    totals.total_amount > 0,
                    least(toFloat64(ar.amount) / totals.total_amount, 1.0),
                    0.0
                ) AS amount_share,
                toUInt8(exchange_wallets.address != '') AS service_mediated
            FROM address_relationships_canonical AS ar
            INNER JOIN token_totals AS totals
                ON totals.token_address = ar.token_address
            LEFT JOIN exchange_wallets
                ON exchange_wallets.address = ar.to_address
            WHERE ar.from_address = ?
              AND ar.transfer_type IN ('native_transfer', 'trc10_transfer', 'trc20_transfer')
              AND (? = 0 OR (ar.token_address = ? AND ar.block_number > ?))
            ORDER BY ar.block_number DESC, ar.tx_hash ASC
            LIMIT ?
            "#,
        )
        .bind(address)
        .bind(address)
        .bind(has_arrival)
        .bind(asset)
        .bind(after_block)
        .bind(MAX_EDGES_PER_ADDRESS + 1)
        .fetch_all::<ExposureEdgeRow>()
        .await?;
    ensure!(
        rows.len() <= MAX_EDGES_PER_ADDRESS as usize,
        "exposure edge limit reached; refusing to publish a partial run as complete"
    );
    if !rows.is_empty() {
        // Validate only this bounded batch, not a set containing the entire chain's block numbers.
        let blocks = rows.iter().map(|row| row.block_number).collect::<Vec<_>>();
        let complete = clickhouse.query("SELECT block_number FROM ingested_blocks FINAL WHERE chain='tron' AND ingestion_status='COMPLETE' AND block_number IN ?")
            .bind(blocks).fetch_all::<u64>().await?.into_iter().collect::<std::collections::HashSet<_>>();
        rows.retain(|row| complete.contains(&row.block_number));
    }
    Ok(rows)
}

fn time_weight(now_unix_ms: u64, event_unix_ms: u64) -> f64 {
    const HALF_LIFE_MS: f64 = 180.0 * 24.0 * 60.0 * 60.0 * 1_000.0;

    let age_ms = now_unix_ms.saturating_sub(event_unix_ms) as f64;
    0.5_f64.powf(age_ms / HALF_LIFE_MS).clamp(0.05, 1.0)
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
