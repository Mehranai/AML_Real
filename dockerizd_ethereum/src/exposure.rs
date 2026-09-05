use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, ensure};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{config::AppConfig, db::database_client};

const DETECTOR_VERSION: &str = "ethereum_exposure_propagation_v1";

#[derive(Debug, Clone, Copy)]
pub struct ExposureOptions {
    pub max_hops: u8,
    pub hop_decay: f64,
    pub time_half_life_days: f64,
    pub max_paths_per_subject: u16,
}

impl Default for ExposureOptions {
    fn default() -> Self {
        Self {
            max_hops: 5,
            hop_decay: 0.65,
            time_half_life_days: 365.0,
            max_paths_per_subject: 3,
        }
    }
}

impl ExposureOptions {
    fn validate(self) -> anyhow::Result<Self> {
        ensure!((1..=10).contains(&self.max_hops), "max_hops must be 1..=10");
        ensure!(
            self.hop_decay > 0.0 && self.hop_decay <= 1.0,
            "hop_decay must be greater than 0 and at most 1"
        );
        ensure!(
            self.time_half_life_days > 0.0,
            "time_half_life_days must be positive"
        );
        ensure!(
            (1..=25).contains(&self.max_paths_per_subject),
            "max_paths_per_subject must be 1..=25"
        );
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExposurePropagationReport {
    pub run_id: String,
    pub seed_count: u64,
    pub path_count: u64,
    pub max_hops: u8,
}

#[derive(Debug, Deserialize, Row)]
struct CountRow {
    count: u64,
}

pub async fn propagate_exposure(
    config: &AppConfig,
    options: ExposureOptions,
) -> anyhow::Result<ExposurePropagationReport> {
    let options = options.validate()?;
    let client = database_client(config);
    let started_at = unix_time_millis()?;
    let run_id = stable_id(
        "eth_exposure_run",
        &[&config.eth_network_id, &started_at.to_string()],
    );

    write_run(
        &client, config, &run_id, "RUNNING", options, 0, 0, started_at, 0, "",
    )
    .await?;

    let result = propagate(&client, config, &run_id, options, started_at).await;
    match result {
        Ok(report) => {
            write_run(
                &client,
                config,
                &run_id,
                "COMPLETE",
                options,
                report.seed_count,
                report.path_count,
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
                options,
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

async fn propagate(
    client: &Client,
    config: &AppConfig,
    run_id: &str,
    options: ExposureOptions,
    created_at: u64,
) -> anyhow::Result<ExposurePropagationReport> {
    seed_paths(client, config, run_id, created_at).await?;
    let seed_count = client
        .query(
            r#"
            SELECT count() AS count
            FROM address_exposure_paths
            WHERE network_id = ? AND run_id = ? AND direction = 'SEED'
            "#,
        )
        .bind(&config.eth_network_id)
        .bind(run_id)
        .fetch_one::<CountRow>()
        .await
        .context("failed to count Ethereum exposure seeds")?
        .count;

    ensure!(
        seed_count > 0,
        "no approved address_entities rows are marked is_exposure_seed=1"
    );

    for hop in 1..=options.max_hops {
        expand_hop(
            client,
            config,
            run_id,
            options,
            created_at,
            hop,
            ExposureDirection::ReceivedFromSeed,
        )
        .await?;
        expand_hop(
            client,
            config,
            run_id,
            options,
            created_at,
            hop,
            ExposureDirection::SentToSeed,
        )
        .await?;
    }

    let path_count = client
        .query(
            r#"
            SELECT count() AS count
            FROM address_exposure_paths
            WHERE network_id = ? AND run_id = ? AND direction != 'SEED'
            "#,
        )
        .bind(&config.eth_network_id)
        .bind(run_id)
        .fetch_one::<CountRow>()
        .await
        .context("failed to count Ethereum exposure paths")?
        .count;

    Ok(ExposurePropagationReport {
        run_id: run_id.to_string(),
        seed_count,
        path_count,
        max_hops: options.max_hops,
    })
}

async fn seed_paths(
    client: &Client,
    config: &AppConfig,
    run_id: &str,
    created_at: u64,
) -> anyhow::Result<()> {
    client
        .query(
            r#"
            INSERT INTO address_exposure_paths
            (
                path_id, run_id, network_id, subject_address, seed_address,
                seed_entity_id, seed_category, seed_risk_level, direction,
                hop_count, asset_id, exposure_score, amount_share, time_weight,
                service_mediated, service_entity_id, continuity_type,
                path_addresses, relationship_ids, tx_hashes,
                first_transfer_unix_ms, last_transfer_unix_ms,
                detector_version, created_at_unix_ms
            )
            SELECT
                hex(SHA256(concat('exposure_seed|', ?, '|', address, '|', entity_id))),
                ?, network_id, address, address, entity_id, seed_category, risk_level,
                'SEED', 0, '', toFloat64(risk_level) / 100.0, 1.0, 1.0,
                0, '', 'SEED', [address], [], [], 0, 0, ?, ?
            FROM address_entities_active
            WHERE network_id = ?
              AND is_exposure_seed = 1
              AND risk_level > 0
            "#,
        )
        .bind(run_id)
        .bind(run_id)
        .bind(DETECTOR_VERSION)
        .bind(created_at)
        .bind(&config.eth_network_id)
        .execute()
        .await
        .context("failed to initialize Ethereum exposure seeds")
}

#[derive(Debug, Clone, Copy)]
enum ExposureDirection {
    ReceivedFromSeed,
    SentToSeed,
}

impl ExposureDirection {
    fn value(self) -> &'static str {
        match self {
            Self::ReceivedFromSeed => "RECEIVED_FROM_SEED",
            Self::SentToSeed => "SENT_TO_SEED",
        }
    }

    fn edge_from(self) -> &'static str {
        match self {
            Self::ReceivedFromSeed => "from_address",
            Self::SentToSeed => "to_address",
        }
    }

    fn edge_to(self) -> &'static str {
        match self {
            Self::ReceivedFromSeed => "to_address",
            Self::SentToSeed => "from_address",
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn expand_hop(
    client: &Client,
    config: &AppConfig,
    run_id: &str,
    options: ExposureOptions,
    created_at: u64,
    hop: u8,
    direction: ExposureDirection,
) -> anyhow::Result<()> {
    let previous_hop = hop - 1;
    let edge_from = direction.edge_from();
    let edge_to = direction.edge_to();
    let direction_value = direction.value();
    let max_paths = options.max_paths_per_subject;
    let hop_decay = options.hop_decay;
    let half_life = options.time_half_life_days;

    let sql = format!(
        r#"
        INSERT INTO address_exposure_paths
        (
            path_id, run_id, network_id, subject_address, seed_address,
            seed_entity_id, seed_category, seed_risk_level, direction,
            hop_count, asset_id, exposure_score, amount_share, time_weight,
            service_mediated, service_entity_id, continuity_type,
            path_addresses, relationship_ids, tx_hashes,
            first_transfer_unix_ms, last_transfer_unix_ms,
            detector_version, created_at_unix_ms
        )
        SELECT
            hex(SHA256(concat(
                'exposure_path|', ?, '|', p.seed_address, '|', edge.relationship_id,
                '|', arrayStringConcat(p.relationship_ids, ':')
            ))),
            ?, p.network_id, edge.{edge_to}, p.seed_address,
            p.seed_entity_id, p.seed_category, p.seed_risk_level,
            '{direction_value}', toUInt8({hop}),
            if(p.asset_id = '', edge.asset_id, p.asset_id),
            (toFloat64(p.seed_risk_level) / 100.0)
                * pow({hop_decay}, {hop})
                * least(p.amount_share, edge.edge_share)
                * least(
                    p.time_weight,
                    exp(
                        -log(2.0)
                        * greatest(
                            0.0,
                            (toFloat64({created_at}) - toFloat64(edge.block_timestamp_unix_ms))
                                / 86400000.0
                        )
                        / {half_life}
                    )
                ),
            least(p.amount_share, edge.edge_share),
            least(
                p.time_weight,
                exp(
                    -log(2.0)
                    * greatest(
                        0.0,
                        (toFloat64({created_at}) - toFloat64(edge.block_timestamp_unix_ms))
                            / 86400000.0
                    )
                    / {half_life}
                )
            ),
            greatest(p.service_mediated, toUInt8(next_service.address != '')),
            if(next_service.address = '', p.service_entity_id, next_service.service_entity_id),
            'DIRECT_ASSET',
            arrayConcat(p.path_addresses, [edge.{edge_to}]),
            arrayConcat(p.relationship_ids, [edge.relationship_id]),
            arrayConcat(p.tx_hashes, [edge.tx_hash]),
            if(p.first_transfer_unix_ms = 0, edge.block_timestamp_unix_ms, p.first_transfer_unix_ms),
            edge.block_timestamp_unix_ms,
            ?, {created_at}
        FROM address_exposure_paths AS p
        INNER JOIN
        (
            SELECT
                relationship_id, tx_hash, block_timestamp_unix_ms,
                from_address, to_address, asset_id,
                least(
                    1.0,
                    toFloat64(toString(amount))
                        / nullIf(
                            sum(toFloat64(toString(amount))) OVER
                                (PARTITION BY {edge_from}, asset_id),
                            0.0
                        )
                ) AS edge_share
            FROM address_relationships_canonical
            WHERE network_id = ?
              AND {edge_from} IN
              (
                  SELECT subject_address
                  FROM address_exposure_paths
                  WHERE network_id = ? AND run_id = ? AND hop_count = {previous_hop}
              )
        ) AS edge
            ON p.subject_address = edge.{edge_from}
        ANY LEFT JOIN
        (
            SELECT network_id, address, any(service_entity_id) AS service_entity_id
            FROM service_boundary_addresses
            GROUP BY network_id, address
        ) AS current_service
            ON current_service.network_id = p.network_id
           AND current_service.address = p.subject_address
        ANY LEFT JOIN
        (
            SELECT network_id, address, any(service_entity_id) AS service_entity_id
            FROM service_boundary_addresses
            GROUP BY network_id, address
        ) AS next_service
            ON next_service.network_id = p.network_id
           AND next_service.address = edge.{edge_to}
        WHERE p.network_id = ?
          AND p.run_id = ?
          AND p.hop_count = {previous_hop}
          AND p.direction IN ('SEED', '{direction_value}')
          AND (p.asset_id = '' OR p.asset_id = edge.asset_id)
          AND NOT has(p.path_addresses, edge.{edge_to})
          AND ({previous_hop} = 0 OR current_service.address = '')
        ORDER BY exposure_score DESC, last_transfer_unix_ms DESC
        LIMIT {max_paths} BY seed_address, subject_address, direction, hop_count
        "#
    );

    client
        .query(&sql)
        .bind(run_id)
        .bind(run_id)
        .bind(DETECTOR_VERSION)
        .bind(&config.eth_network_id)
        .bind(&config.eth_network_id)
        .bind(run_id)
        .bind(&config.eth_network_id)
        .bind(run_id)
        .execute()
        .await
        .with_context(|| {
            format!(
                "failed to propagate Ethereum exposure direction {} at hop {}",
                direction_value, hop
            )
        })
}

#[allow(clippy::too_many_arguments)]
async fn write_run(
    client: &Client,
    config: &AppConfig,
    run_id: &str,
    status: &str,
    options: ExposureOptions,
    seed_count: u64,
    path_count: u64,
    started_at: u64,
    completed_at: u64,
    error_message: &str,
) -> anyhow::Result<()> {
    client
        .query(
            r#"
            INSERT INTO exposure_runs
            (
                run_id, network_id, detector_version, status, max_hops, hop_decay,
                time_half_life_days, max_paths_per_subject, seed_count, path_count,
                started_at_unix_ms, completed_at_unix_ms, error_message
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(run_id)
        .bind(&config.eth_network_id)
        .bind(DETECTOR_VERSION)
        .bind(status)
        .bind(options.max_hops)
        .bind(options.hop_decay)
        .bind(options.time_half_life_days)
        .bind(options.max_paths_per_subject)
        .bind(seed_count)
        .bind(path_count)
        .bind(started_at)
        .bind(completed_at)
        .bind(error_message)
        .execute()
        .await
        .context("failed to persist Ethereum exposure run")
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
    use super::ExposureOptions;

    #[test]
    fn validates_exposure_bounds() {
        assert!(ExposureOptions::default().validate().is_ok());
        assert!(
            ExposureOptions {
                max_hops: 11,
                ..ExposureOptions::default()
            }
            .validate()
            .is_err()
        );
    }
}
