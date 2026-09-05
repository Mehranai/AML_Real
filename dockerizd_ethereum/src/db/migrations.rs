use std::collections::HashMap;

use anyhow::{Context, anyhow, ensure};
use clickhouse::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::config::AppConfig;

use super::sql::run_sql;

struct SchemaMigration {
    id: &'static str,
    description: &'static str,
    sql_template: &'static str,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct AppliedMigration {
    migration_id: String,
    checksum: String,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct StoredColumn {
    name: String,
    r#type: String,
}

pub fn database_client(config: &AppConfig) -> Client {
    Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_password)
        .with_database(&config.clickhouse_database)
}

fn admin_client(config: &AppConfig) -> Client {
    Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_password)
}

pub async fn initialize_ethereum_schema(config: &AppConfig) -> anyhow::Result<()> {
    let client = admin_client(config);
    let database = &config.clickhouse_database;

    client
        .query(&format!("CREATE DATABASE IF NOT EXISTS {database}"))
        .execute()
        .await
        .with_context(|| format!("failed to create ClickHouse database {database}"))?;

    ensure_migration_ledger(&client, database).await?;
    reject_legacy_schema_collisions(&client, database).await?;
    apply_migrations(&client, database).await?;
    validate_schema(&client, database).await?;

    Ok(())
}

async fn reject_legacy_schema_collisions(client: &Client, database: &str) -> anyhow::Result<()> {
    const CANONICAL_MARKERS: &[(&str, &str)] = &[
        ("transactions", "tx_hash"),
        ("token_metadata", "network_id"),
        ("sync_state", "network_id"),
    ];

    for (table, marker_column) in CANONICAL_MARKERS {
        let existing_columns = client
            .query(
                r#"
                SELECT name, type
                FROM system.columns
                WHERE database = ? AND table = ?
                "#,
            )
            .bind(database)
            .bind(*table)
            .fetch_all::<StoredColumn>()
            .await
            .with_context(|| format!("failed to inspect {database}.{table} before migration"))?;

        if !existing_columns.is_empty()
            && !existing_columns
                .iter()
                .any(|column| column.name == *marker_column)
        {
            return Err(anyhow!(
                "legacy schema collision at {database}.{table}: expected canonical marker column \
                 {marker_column}; choose an empty CLICKHOUSE_DB_ETH database or migrate the old \
                 table explicitly"
            ));
        }
    }

    Ok(())
}

async fn ensure_migration_ledger(client: &Client, database: &str) -> anyhow::Result<()> {
    client
        .query(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {database}.schema_migrations
            (
                migration_id String,
                description String,
                checksum String,
                applied_at DateTime64(3) DEFAULT now64(3)
            )
            ENGINE = ReplacingMergeTree(applied_at)
            ORDER BY migration_id
            "#
        ))
        .execute()
        .await
        .context("failed to create Ethereum schema migration ledger")?;

    Ok(())
}

async fn apply_migrations(client: &Client, database: &str) -> anyhow::Result<()> {
    let applied = client
        .query(&format!(
            r#"
            SELECT migration_id, argMax(checksum, applied_at) AS checksum
            FROM {database}.schema_migrations
            GROUP BY migration_id
            "#
        ))
        .fetch_all::<AppliedMigration>()
        .await
        .context("failed to load Ethereum schema migration ledger")?;

    for migration in migrations() {
        let checksum = migration_checksum(migration.sql_template);

        if let Some(existing) = applied
            .iter()
            .find(|item| item.migration_id == migration.id)
        {
            ensure!(
                existing.checksum == checksum,
                "Ethereum schema migration {} checksum changed; create a new migration instead",
                migration.id
            );
            continue;
        }

        tracing::info!(
            migration_id = migration.id,
            description = migration.description,
            "applying Ethereum schema migration"
        );

        let sql = render_migration_sql(migration.sql_template, database)?;
        run_sql(client, &sql)
            .await
            .with_context(|| format!("failed to apply migration {}", migration.id))?;

        client
            .query(&format!(
                r#"
                INSERT INTO {database}.schema_migrations
                    (migration_id, description, checksum)
                VALUES (?, ?, ?)
                "#
            ))
            .bind(migration.id)
            .bind(migration.description)
            .bind(checksum)
            .execute()
            .await
            .with_context(|| format!("failed to record migration {}", migration.id))?;
    }

    Ok(())
}

fn migrations() -> &'static [SchemaMigration] {
    &[
        SchemaMigration {
            id: "20260810_0001_ethereum_canonical_schema",
            description: "Create lean replay-safe Ethereum canonical evidence schema",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260810_0001_canonical_schema.sql"
            ),
        },
        SchemaMigration {
            id: "20260810_0002_ingestion_completeness",
            description: "Record receipt and trace evidence completeness for ingested blocks",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260810_0002_ingestion_completeness.sql"
            ),
        },
        SchemaMigration {
            id: "20260812_0003_semantic_protocols",
            description: "Add ERC-1155 item identity and versioned protocol registry evidence",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260812_0003_semantic_protocols.sql"
            ),
        },
        SchemaMigration {
            id: "20260812_0004_refresh_canonical_views",
            description: "Refresh canonical views after semantic schema expansion",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260812_0004_refresh_canonical_views.sql"
            ),
        },
        SchemaMigration {
            id: "20260812_0005_deduplicate_bridge_events",
            description: "Deduplicate equivalent bridge ABI events canonically",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260812_0005_deduplicate_bridge_events.sql"
            ),
        },
        SchemaMigration {
            id: "20260829_0006_semantic_v2",
            description: "Activate mixer/liquidity evidence and token-standard metadata jobs",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260829_0006_semantic_v2.sql"
            ),
        },
        SchemaMigration {
            id: "20260829_0007_metadata_intelligence",
            description: "Add metadata completeness and reviewed entity intelligence",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260829_0007_metadata_intelligence.sql"
            ),
        },
        SchemaMigration {
            id: "20260829_0008_clustering_exposure",
            description: "Add reviewed clustering evidence and explainable exposure paths",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260829_0008_clustering_exposure.sql"
            ),
        },
        SchemaMigration {
            id: "20260829_0009_evidence_risk",
            description: "Add versioned explainable risk policy, signals, and assessments",
            sql_template: include_str!(
                "../../sql/ethereum_migration_20260829_0009_evidence_risk.sql"
            ),
        },
    ]
}

fn migration_checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

fn render_migration_sql(template: &str, database: &str) -> anyhow::Result<String> {
    ensure!(
        is_safe_identifier(database),
        "unsafe ClickHouse database name"
    );
    let rendered = template.replace("{{database}}", database);
    ensure!(
        !rendered.contains("{{database}}"),
        "unresolved database placeholder in Ethereum migration"
    );

    Ok(rendered)
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

async fn validate_schema(client: &Client, database: &str) -> anyhow::Result<()> {
    for required in required_tables() {
        let columns = client
            .query(
                r#"
                SELECT name, type
                FROM system.columns
                WHERE database = ? AND table = ?
                "#,
            )
            .bind(database)
            .bind(required.table)
            .fetch_all::<StoredColumn>()
            .await
            .with_context(|| format!("failed to inspect {}.{}", database, required.table))?;

        if columns.is_empty() {
            return Err(anyhow!(
                "required Ethereum table {}.{} does not exist",
                database,
                required.table
            ));
        }

        let stored = columns
            .into_iter()
            .map(|column| (column.name, column.r#type))
            .collect::<HashMap<_, _>>();

        for (name, expected_type) in required.columns {
            let Some(actual_type) = stored.get(*name) else {
                return Err(anyhow!(
                    "required column {}.{}.{} does not exist",
                    database,
                    required.table,
                    name
                ));
            };
            ensure!(
                actual_type == expected_type,
                "column {}.{}.{} has type {}, expected {}",
                database,
                required.table,
                name,
                actual_type,
                expected_type
            );
        }
    }

    Ok(())
}

struct RequiredTable {
    table: &'static str,
    columns: &'static [(&'static str, &'static str)],
}

fn required_tables() -> &'static [RequiredTable] {
    &[
        RequiredTable {
            table: "ingested_blocks",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("block_number", "UInt64"),
                ("block_hash", "String"),
                ("ingestion_status", "LowCardinality(String)"),
                ("receipt_data_complete", "UInt8"),
                ("trace_data_complete", "UInt8"),
                ("rpc_provider", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "transactions",
            columns: &[
                ("tx_hash", "String"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("status", "UInt8"),
                ("fee_paid", "UInt256"),
                ("input_data", "String"),
            ],
        },
        RequiredTable {
            table: "evm_logs",
            columns: &[
                ("event_id", "String"),
                ("topics", "Array(String)"),
                ("data", "String"),
            ],
        },
        RequiredTable {
            table: "address_relationships",
            columns: &[
                ("relationship_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("trace_address", "Array(UInt32)"),
                ("event_sub_index", "UInt32"),
                ("amount", "UInt256"),
                ("transfer_type", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "transaction_features",
            columns: &[
                ("feature_id", "String"),
                ("detector_version", "String"),
                ("is_mixer", "UInt8"),
            ],
        },
        RequiredTable {
            table: "semantic_aml_events",
            columns: &[
                ("event_id", "String"),
                ("evidence_json", "String"),
                ("protocol_contract", "String"),
                ("correlation_key", "String"),
            ],
        },
        RequiredTable {
            table: "token_metadata",
            columns: &[
                ("token_address", "String"),
                ("token_standard", "LowCardinality(String)"),
                ("decimals_known", "UInt8"),
                ("metadata_status", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "entity_labels",
            columns: &[
                ("label_id", "String"),
                ("review_status", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "address_entities",
            columns: &[
                ("address", "String"),
                ("risk_level", "UInt8"),
                ("is_exposure_seed", "UInt8"),
                ("seed_category", "LowCardinality(String)"),
                ("is_active", "UInt8"),
            ],
        },
        RequiredTable {
            table: "address_cluster_memberships",
            columns: &[
                ("address", "String"),
                ("cluster_id", "String"),
                ("membership_type", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "address_exposure_paths",
            columns: &[
                ("subject_address", "String"),
                ("exposure_score", "Float64"),
                ("path_addresses", "Array(String)"),
                ("relationship_ids", "Array(String)"),
            ],
        },
        RequiredTable {
            table: "risk_policy_rules",
            columns: &[
                ("signal_type", "LowCardinality(String)"),
                ("max_contribution", "Float64"),
                ("enabled", "UInt8"),
            ],
        },
        RequiredTable {
            table: "wallet_risk_assessments",
            columns: &[
                ("risk_score", "Float64"),
                ("risk_level", "LowCardinality(String)"),
                ("top_reasons", "Array(String)"),
            ],
        },
        RequiredTable {
            table: "protocol_contract_registry",
            columns: &[
                ("contract_address", "String"),
                ("decoder", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "ingestion_failures",
            columns: &[
                ("failure_id", "String"),
                ("status", "LowCardinality(String)"),
            ],
        },
        RequiredTable {
            table: "sync_state",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("last_synced_block", "UInt64"),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{is_safe_identifier, migration_checksum, migrations, render_migration_sql};

    #[test]
    fn renders_database_placeholder() {
        let sql = render_migration_sql("CREATE TABLE {{database}}.events (id UInt64);", "eth_db")
            .unwrap();
        assert_eq!(sql, "CREATE TABLE eth_db.events (id UInt64);");
    }

    #[test]
    fn rejects_unsafe_database_name() {
        assert!(!is_safe_identifier("eth_db; DROP DATABASE eth_db"));
        assert!(render_migration_sql("SELECT 1", "eth-db").is_err());
    }

    #[test]
    fn migration_ids_and_checksums_are_unique() {
        let migrations = migrations();
        let ids = migrations
            .iter()
            .map(|item| item.id)
            .collect::<std::collections::HashSet<_>>();
        let checksums = migrations
            .iter()
            .map(|item| migration_checksum(item.sql_template))
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(ids.len(), migrations.len());
        assert_eq!(checksums.len(), migrations.len());
    }
}
