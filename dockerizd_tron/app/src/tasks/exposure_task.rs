use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use clickhouse::Client;
use nanoid::nanoid;
use serde::Deserialize;
use serde::Serialize;

use crate::models::tron::exposure::AddressExposureRow;
use crate::services::tron::exposure::propagation::propagate_exposure;

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ExposureSeedAddress {
    address: String,
}

#[derive(Debug, Serialize, clickhouse::Row)]
struct ExposureRunRow {
    source_address: String,
    propagation_run_id: String,
    status: String,
    max_hops: u8,
    row_count: u64,
    completed_at_unix_ms: u64,
}

pub async fn run_exposure_scan(clickhouse: Arc<Client>, seed: &str, max_hops: u8) -> Result<usize> {
    let propagation_run_id = format!("tron_exposure_{}", nanoid!(16));
    let completed_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let rows = propagate_exposure(&clickhouse, seed, max_hops, &propagation_run_id).await?;
    let row_count = rows.len();
    if !rows.is_empty() {
        let mut insert = clickhouse
            .insert::<AddressExposureRow>("address_exposure")
            .await?;

        for row in rows {
            insert.write(&row).await?;
        }
        insert.end().await?;
    }

    let mut run_insert = clickhouse.insert::<ExposureRunRow>("exposure_runs").await?;
    run_insert
        .write(&ExposureRunRow {
            source_address: seed.to_string(),
            propagation_run_id,
            status: "COMPLETE".to_string(),
            max_hops,
            row_count: row_count as u64,
            completed_at_unix_ms,
        })
        .await?;
    run_insert.end().await?;

    Ok(row_count)
}

pub async fn run_all_exposure_scans(
    clickhouse: Arc<Client>,
    max_hops: u8,
) -> Result<(usize, usize)> {
    let seeds = clickhouse
        .query(
            r#"
            SELECT address
            FROM exposure_seeds FINAL
            WHERE address != ''
              AND risk_level > 0
              AND is_active = 1
            ORDER BY address
            "#,
        )
        .fetch_all::<ExposureSeedAddress>()
        .await?;
    let seed_count = seeds.len();
    let mut exposure_count = 0usize;

    for seed in seeds {
        let count = run_exposure_scan(clickhouse.clone(), &seed.address, max_hops).await?;
        exposure_count = exposure_count.saturating_add(count);
        println!(
            "[TRON EXPOSURE] seed={} propagated_rows={}",
            seed.address, count
        );
    }

    Ok((seed_count, exposure_count))
}
