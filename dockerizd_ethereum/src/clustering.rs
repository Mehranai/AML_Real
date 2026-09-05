use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, ensure};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{config::AppConfig, db::database_client};

const DETECTOR_VERSION: &str = "ethereum_address_clustering_v1";

#[derive(Debug, Clone, Serialize)]
pub struct ClusterDiscoveryReport {
    pub run_id: String,
    pub entity_memberships: u64,
    pub reviewed_entity_claims: u64,
    pub control_claims: u64,
}

#[derive(Debug, Deserialize, Row)]
struct CountRow {
    count: u64,
}

pub async fn discover_address_clusters(
    config: &AppConfig,
) -> anyhow::Result<ClusterDiscoveryReport> {
    let client = database_client(config);
    let started_at = unix_time_millis()?;
    let run_id = stable_id(
        "eth_cluster_run",
        &[&config.eth_network_id, &started_at.to_string()],
    );

    write_run(&client, config, &run_id, "RUNNING", 0, 0, started_at, 0, "").await?;

    let result = discover(&client, config, &run_id, started_at).await;
    match result {
        Ok(report) => {
            write_run(
                &client,
                config,
                &run_id,
                "COMPLETE",
                report.entity_memberships,
                report.control_claims,
                started_at,
                unix_time_millis()?,
                "",
            )
            .await?;
            Ok(report)
        }
        Err(error) => {
            write_run(
                &client,
                config,
                &run_id,
                "FAILED",
                0,
                0,
                started_at,
                unix_time_millis()?,
                &error.to_string(),
            )
            .await?;
            Err(error)
        }
    }
}

async fn discover(
    client: &Client,
    config: &AppConfig,
    run_id: &str,
    created_at: u64,
) -> anyhow::Result<ClusterDiscoveryReport> {
    client
        .query(
            r#"
            INSERT INTO address_cluster_claims
            (
                claim_id, run_id, network_id, left_address, right_address, cluster_id,
                claim_type, same_entity, confidence, review_status, source_id,
                evidence_tx_hash, evidence_refs, detector, detector_version,
                evidence_json, created_at_unix_ms
            )
            SELECT
                hex(SHA256(concat('reviewed_entity|', network_id, '|', entity_id, '|', address))),
                ?, network_id, anchor_address, address,
                hex(SHA256(concat('entity_cluster|', network_id, '|', entity_id))),
                'REVIEWED_ENTITY', 1, confidence, 'APPROVED', source_label_id,
                '', [source_label_id], 'entity_intelligence', ?,
                concat('{"entity_id":"', entity_id, '","source_label_id":"', source_label_id, '"}'),
                ?
            FROM
            (
                SELECT
                    *,
                    min(address) OVER (PARTITION BY network_id, entity_id) AS anchor_address
                FROM address_entities_active
                WHERE network_id = ?
            )
            WHERE address != anchor_address
            "#,
        )
        .bind(run_id)
        .bind(DETECTOR_VERSION)
        .bind(created_at)
        .bind(&config.eth_network_id)
        .execute()
        .await
        .context("failed to create reviewed-entity cluster claims")?;

    client
        .query(
            r#"
            INSERT INTO address_cluster_memberships
            (
                membership_id, run_id, network_id, address, cluster_id, entity_id,
                membership_type, confidence, source_claim_id, is_active,
                created_at_unix_ms
            )
            SELECT
                hex(SHA256(concat('entity_membership|', network_id, '|', entity_id, '|', address))),
                ?, network_id, address,
                hex(SHA256(concat('entity_cluster|', network_id, '|', entity_id))),
                entity_id, 'REVIEWED_ENTITY', confidence,
                hex(SHA256(concat('reviewed_entity|', network_id, '|', entity_id, '|', address))),
                1, ?
            FROM address_entities_active
            WHERE network_id = ?
            "#,
        )
        .bind(run_id)
        .bind(created_at)
        .bind(&config.eth_network_id)
        .execute()
        .await
        .context("failed to create reviewed-entity cluster memberships")?;

    // A deployment proves a control relationship at that transaction, not common ownership.
    // It is retained as reviewable evidence and deliberately excluded from memberships.
    client
        .query(
            r#"
            INSERT INTO address_cluster_claims
            (
                claim_id, run_id, network_id, left_address, right_address, cluster_id,
                claim_type, same_entity, confidence, review_status, source_id,
                evidence_tx_hash, evidence_refs, detector, detector_version,
                evidence_json, created_at_unix_ms
            )
            SELECT
                hex(SHA256(concat('contract_deployment|', network_id, '|', tx_hash))),
                ?, network_id, from_address, contract_address, '',
                'CONTRACT_DEPLOYMENT_CONTROL', 0, toFloat32(1), 'AUTO_EVIDENCE',
                'ethereum_canonical_ingestion', tx_hash, [tx_hash],
                'contract_creation_receipt', ?,
                concat('{"block_number":', toString(block_number), ',"tx_hash":"', tx_hash, '"}'),
                ?
            FROM transactions_canonical
            WHERE network_id = ?
              AND contract_address != ''
              AND status_known = 1
              AND status = 1
            "#,
        )
        .bind(run_id)
        .bind(DETECTOR_VERSION)
        .bind(created_at)
        .bind(&config.eth_network_id)
        .execute()
        .await
        .context("failed to create contract-deployment control claims")?;

    let entity_memberships = count_run_rows(
        client,
        "address_cluster_memberships",
        run_id,
        &config.eth_network_id,
    )
    .await?;
    let reviewed_entity_claims =
        count_claim_type(client, run_id, &config.eth_network_id, "REVIEWED_ENTITY").await?;
    let control_claims = count_claim_type(
        client,
        run_id,
        &config.eth_network_id,
        "CONTRACT_DEPLOYMENT_CONTROL",
    )
    .await?;

    Ok(ClusterDiscoveryReport {
        run_id: run_id.to_string(),
        entity_memberships,
        reviewed_entity_claims,
        control_claims,
    })
}

async fn count_run_rows(
    client: &Client,
    table: &str,
    run_id: &str,
    network_id: &str,
) -> anyhow::Result<u64> {
    ensure!(
        matches!(
            table,
            "address_cluster_memberships" | "address_cluster_claims"
        ),
        "unsupported count table"
    );
    let sql = format!("SELECT count() AS count FROM {table} WHERE network_id = ? AND run_id = ?");
    Ok(client
        .query(&sql)
        .bind(network_id)
        .bind(run_id)
        .fetch_one::<CountRow>()
        .await?
        .count)
}

async fn count_claim_type(
    client: &Client,
    run_id: &str,
    network_id: &str,
    claim_type: &str,
) -> anyhow::Result<u64> {
    Ok(client
        .query(
            r#"
            SELECT count() AS count
            FROM address_cluster_claims
            WHERE network_id = ? AND run_id = ? AND claim_type = ?
            "#,
        )
        .bind(network_id)
        .bind(run_id)
        .bind(claim_type)
        .fetch_one::<CountRow>()
        .await?
        .count)
}

#[allow(clippy::too_many_arguments)]
async fn write_run(
    client: &Client,
    config: &AppConfig,
    run_id: &str,
    status: &str,
    entity_memberships: u64,
    control_claims: u64,
    started_at: u64,
    completed_at: u64,
    error_message: &str,
) -> anyhow::Result<()> {
    client
        .query(
            r#"
            INSERT INTO cluster_runs
            (
                run_id, network_id, detector_version, status, entity_memberships,
                control_claims, started_at_unix_ms, completed_at_unix_ms, error_message
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(run_id)
        .bind(&config.eth_network_id)
        .bind(DETECTOR_VERSION)
        .bind(status)
        .bind(entity_memberships)
        .bind(control_claims)
        .bind(started_at)
        .bind(completed_at)
        .bind(error_message)
        .execute()
        .await
        .context("failed to persist Ethereum cluster run")
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
