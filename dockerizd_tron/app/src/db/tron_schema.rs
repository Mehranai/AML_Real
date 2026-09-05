use anyhow::{Context, anyhow};
use clickhouse::Client;
use serde::Deserialize;

const TRON_DB: &str = "tron_db";
const BASELINE_SQL: &str = include_str!("../../sql/init_database_tron.sql");
const CREATE_PREFIXES: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS tron_db.",
    "CREATE VIEW IF NOT EXISTS tron_db.",
    "CREATE MATERIALIZED VIEW IF NOT EXISTS tron_db.",
];

pub async fn validate_tron_schema(client: &Client) -> anyhow::Result<()> {
    validate_baseline_schemas(client).await?;
    validate_retired_objects_absent(client).await?;
    Ok(())
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ColumnInfo {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableSchema {
    table: String,
    columns: Vec<(String, String)>,
}

async fn validate_baseline_schemas(client: &Client) -> anyhow::Result<()> {
    let schemas = baseline_tron_schemas()?;
    let mut incompatible_objects = Vec::new();

    for schema in &schemas {
        let actual = load_columns(client, &schema.table).await?;

        if actual.is_empty() {
            incompatible_objects.push(format!("{} (missing)", schema.table));
        } else if !schema_matches(&actual, &schema.columns) {
            incompatible_objects.push(format!("{} (exact column mismatch)", schema.table));
        }
    }

    if !incompatible_objects.is_empty() {
        return Err(anyhow!(
            "TRON schema validation failed for baseline objects: {}. Recreate tron_db from sql/init_database_tron.sql",
            incompatible_objects.join(", ")
        ));
    }

    Ok(())
}

fn baseline_tron_schemas() -> anyhow::Result<Vec<TableSchema>> {
    let mut schemas = Vec::new();
    let mut current: Option<TableSchema> = None;

    for line in BASELINE_SQL.lines() {
        let trimmed = line.trim();

        if let Some(table) = CREATE_PREFIXES
            .iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix))
        {
            if current.is_some() {
                return Err(anyhow!("nested TRON schema declaration near {table}"));
            }
            current = Some(TableSchema {
                table: table.split_whitespace().next().unwrap_or(table).to_string(),
                columns: Vec::new(),
            });
            continue;
        }

        let Some(schema) = current.as_mut() else {
            continue;
        };

        if trimmed.starts_with(')') {
            let completed = current
                .take()
                .ok_or_else(|| anyhow!("missing active TRON schema declaration"))?;
            if completed.columns.is_empty() {
                return Err(anyhow!(
                    "TRON schema object {} has no explicit columns",
                    completed.table
                ));
            }
            schemas.push(completed);
            continue;
        }

        if let Some(column) = parse_column_definition(trimmed) {
            schema.columns.push(column);
        }
    }

    if let Some(schema) = current {
        return Err(anyhow!(
            "unterminated TRON schema declaration for {}",
            schema.table
        ));
    }
    if schemas.is_empty() {
        return Err(anyhow!("TRON baseline SQL contains no schema objects"));
    }

    Ok(schemas)
}

fn parse_column_definition(line: &str) -> Option<(String, String)> {
    let definition = line.strip_prefix('`')?;
    let (name, remainder) = definition.split_once('`')?;
    let data_type = remainder.trim_start().split_whitespace().next()?;

    Some((
        name.to_string(),
        data_type.trim_end_matches(',').to_string(),
    ))
}

async fn load_columns(client: &Client, table: &str) -> anyhow::Result<Vec<ColumnInfo>> {
    client
        .query(
            r#"
            SELECT
                name,
                type
            FROM system.columns
            WHERE database = ?
              AND table = ?
            ORDER BY position
            "#,
        )
        .bind(TRON_DB)
        .bind(table)
        .fetch_all::<ColumnInfo>()
        .await
        .with_context(|| format!("failed to inspect ClickHouse schema for {table}"))
}

fn schema_matches(actual: &[ColumnInfo], expected: &[(String, String)]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.name == expected.0 && actual.data_type == expected.1)
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct TableInfo {
    name: String,
}

async fn validate_retired_objects_absent(client: &Client) -> anyhow::Result<()> {
    let objects = client
        .query(
            r#"
            SELECT name
            FROM system.tables
            WHERE database = ?
            "#,
        )
        .bind(TRON_DB)
        .fetch_all::<TableInfo>()
        .await
        .context("failed to inspect retired TRON objects")?;

    let mut retired = objects
        .into_iter()
        .filter_map(|object| is_retired_tron_object(&object.name).then_some(object.name))
        .collect::<Vec<_>>();
    retired.sort();

    if !retired.is_empty() {
        return Err(anyhow!(
            "retired TRON objects are incompatible with the consolidated baseline: {}. Recreate tron_db from sql/init_database_tron.sql before starting the service",
            retired.join(", ")
        ));
    }

    Ok(())
}

pub fn is_retired_tron_object(name: &str) -> bool {
    retired_tron_objects().contains(&name)
        || name.contains("_legacy_")
        || name.starts_with("mv_token_delta_from_legacy_")
        || name.starts_with("mv_token_delta_to_legacy_")
        || name.starts_with("mv_token_balance_legacy_")
}

pub fn retired_tron_objects() -> &'static [&'static str] {
    &[
        "address_behavior",
        "address_clusters",
        "address_tags",
        "address_profiles",
        "address_counterparties",
        "address_token_balance",
        "address_token_delta",
        "aml_events",
        "analysis_jobs",
        "blocks",
        "cluster_edges",
        "contract_calls",
        "contract_interactions",
        "contract_metadata",
        "entity_relationships",
        "exchange_clusters",
        "exchange_deposit_addresses",
        "exchange_entities",
        "exchange_flows",
        "exchange_flows_v2",
        "exposure_paths",
        "flow_edges_hourly",
        "flow_segments",
        "graph_edges",
        "internal_transfers",
        "investigation_cache",
        "method_signatures",
        "mv_token_balance",
        "mv_token_delta_from",
        "mv_token_delta_to",
        "owner_info",
        "raw_logs",
        "schema_lifecycle",
        "schema_migrations",
        "sweep_edges",
        "token_transfers",
        "transaction_risk",
        "wallet_ai_risk_assessments",
        "wallet_asset_balance_deltas",
        "wallet_asset_balance_deltas_v2",
        "wallet_counterparty_fingerprints",
        "wallet_feature_snapshots",
        "wallet_fingerprints",
        "wallet_info",
        "wallet_ml_labels",
        "wallet_ml_training_runs",
        "wallet_risk",
        "wallet_risk_assessments",
        "wallet_state",
        "address_energy_usage",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_parser_reads_every_declared_object_exactly() {
        let schemas = baseline_tron_schemas().expect("baseline SQL should parse");

        assert!(schemas.len() >= 30);
        assert!(schemas.iter().any(|schema| {
            schema.table == "address_relationships_canonical"
                && schema.columns.iter().any(|(name, data_type)| {
                    name == "classification_confidence" && data_type == "Float32"
                })
        }));
        assert!(schemas.iter().any(|schema| {
            schema.table == "wallet_asset_balances"
                && schema
                    .columns
                    .iter()
                    .any(|(name, data_type)| name == "metadata_verified" && data_type == "UInt8")
        }));
    }

    #[test]
    fn retired_objects_are_not_declared_by_the_baseline() {
        let schemas = baseline_tron_schemas().expect("baseline SQL should parse");

        for schema in schemas {
            assert!(!is_retired_tron_object(&schema.table), "{}", schema.table);
        }
    }
}
