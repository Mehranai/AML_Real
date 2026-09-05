use std::collections::{HashMap, HashSet};

use clickhouse::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::ClickHouseConfig;

use super::sql::run_sql;

#[derive(Clone, Copy)]
struct Migration {
    id: &'static str,
    description: &'static str,
    sql_template: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "20260904_0001_core_evidence",
        description: "Create minimal BSC canonical evidence and operational tables",
        sql_template: include_str!("../../sql/migrations/20260904_0001_core_evidence.sql"),
    },
    Migration {
        id: "20260904_0002_canonical_views",
        description: "Create reorg-aware canonical and current read contracts",
        sql_template: include_str!("../../sql/migrations/20260904_0002_canonical_views.sql"),
    },
    Migration {
        id: "20260904_0003_commit_revision_gate",
        description: "Gate every block fact by the exact complete ingestion revision",
        sql_template: include_str!("../../sql/migrations/20260904_0003_commit_revision_gate.sql"),
    },
    Migration {
        id: "20260904_0004_transfer_evidence",
        description: "Revision-gate token discovery evidence emitted by transfer extraction",
        sql_template: include_str!("../../sql/migrations/20260904_0004_transfer_evidence.sql"),
    },
    Migration {
        id: "20260905_0005_ingestion_reliability",
        description: "Remove unused transaction scope from block-level ingestion failures",
        sql_template: include_str!("../../sql/migrations/20260905_0005_ingestion_reliability.sql"),
    },
    Migration {
        id: "20260905_0006_semantic_evidence",
        description: "Add reviewed protocol registry and complete semantic evidence fields",
        sql_template: include_str!("../../sql/migrations/20260905_0006_semantic_evidence.sql"),
    },
];

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

#[derive(Debug, Deserialize, clickhouse::Row)]
struct StoredObject {
    name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SchemaSummary {
    pub database: String,
    pub migration_count: usize,
    pub newly_applied: usize,
    pub current_migration: String,
}

pub async fn initialize_bsc_schema(
    config: &ClickHouseConfig,
) -> Result<SchemaSummary, SchemaError> {
    let admin = admin_client(config);
    initialize_database(&admin, config.database(), MIGRATIONS).await
}

pub async fn validate_bsc_schema(config: &ClickHouseConfig) -> Result<SchemaSummary, SchemaError> {
    let admin = admin_client(config);
    validate_migration_ledger(&admin, config.database(), MIGRATIONS).await?;
    validate_schema_contract(&admin, config.database()).await?;
    Ok(SchemaSummary {
        database: config.database().to_string(),
        migration_count: MIGRATIONS.len(),
        newly_applied: 0,
        current_migration: MIGRATIONS.last().map_or("", |item| item.id).to_string(),
    })
}

fn admin_client(config: &ClickHouseConfig) -> Client {
    Client::default()
        .with_url(config.endpoint())
        .with_user(config.user())
        .with_password(config.password())
}

async fn initialize_database(
    client: &Client,
    database: &str,
    migrations: &[Migration],
) -> Result<SchemaSummary, SchemaError> {
    ensure_safe_database(database)?;
    client
        .query(&format!("CREATE DATABASE IF NOT EXISTS {database}"))
        .execute()
        .await
        .map_err(|source| SchemaError::ClickHouse {
            operation: "create BSC database",
            source,
        })?;
    ensure_migration_ledger(client, database).await?;
    let newly_applied = apply_migrations(client, database, migrations).await?;
    validate_migration_ledger(client, database, migrations).await?;
    if migrations.len() == MIGRATIONS.len()
        && migrations
            .iter()
            .zip(MIGRATIONS)
            .all(|(left, right)| left.id == right.id)
    {
        validate_schema_contract(client, database).await?;
    }
    Ok(SchemaSummary {
        database: database.to_string(),
        migration_count: migrations.len(),
        newly_applied,
        current_migration: migrations.last().map_or("", |item| item.id).to_string(),
    })
}

#[cfg(test)]
pub(crate) async fn initialize_test_database(
    client: &Client,
    database: &str,
) -> Result<SchemaSummary, SchemaError> {
    initialize_database(client, database, MIGRATIONS).await
}

async fn ensure_migration_ledger(client: &Client, database: &str) -> Result<(), SchemaError> {
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
        .map_err(|source| SchemaError::ClickHouse {
            operation: "create migration ledger",
            source,
        })
}

async fn load_applied(
    client: &Client,
    database: &str,
) -> Result<Vec<AppliedMigration>, SchemaError> {
    client
        .query(&format!(
            r#"
            SELECT migration_id, argMax(checksum, applied_at) AS checksum
            FROM {database}.schema_migrations
            GROUP BY migration_id
            "#
        ))
        .fetch_all::<AppliedMigration>()
        .await
        .map_err(|source| SchemaError::ClickHouse {
            operation: "read migration ledger",
            source,
        })
}

async fn apply_migrations(
    client: &Client,
    database: &str,
    migrations: &[Migration],
) -> Result<usize, SchemaError> {
    validate_migration_manifest(migrations)?;
    let applied = load_applied(client, database).await?;
    let mut newly_applied = 0_usize;
    for migration in migrations {
        let checksum = migration_checksum(migration.sql_template);
        if let Some(existing) = applied
            .iter()
            .find(|item| item.migration_id == migration.id)
        {
            if existing.checksum != checksum {
                return Err(SchemaError::ChecksumChanged {
                    migration_id: migration.id.to_string(),
                });
            }
            continue;
        }

        tracing::info!(migration_id = migration.id, "applying BSC schema migration");
        let sql = render_migration_sql(migration.sql_template, database)?;
        run_sql(client, &sql)
            .await
            .map_err(|error| SchemaError::MigrationSql {
                migration_id: migration.id.to_string(),
                statement_index: error.statement_index,
                source: error.source,
            })?;
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
            .bind(&checksum)
            .execute()
            .await
            .map_err(|source| SchemaError::ClickHouse {
                operation: "record applied migration",
                source,
            })?;
        newly_applied += 1;
    }
    Ok(newly_applied)
}

async fn validate_migration_ledger(
    client: &Client,
    database: &str,
    migrations: &[Migration],
) -> Result<(), SchemaError> {
    let applied = load_applied(client, database).await?;
    for migration in migrations {
        let Some(stored) = applied
            .iter()
            .find(|item| item.migration_id == migration.id)
        else {
            return Err(SchemaError::MissingMigration {
                migration_id: migration.id.to_string(),
            });
        };
        if stored.checksum != migration_checksum(migration.sql_template) {
            return Err(SchemaError::ChecksumChanged {
                migration_id: migration.id.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_migration_manifest(migrations: &[Migration]) -> Result<(), SchemaError> {
    let mut ids = HashSet::new();
    for migration in migrations {
        if !ids.insert(migration.id) {
            return Err(SchemaError::DuplicateMigrationId {
                migration_id: migration.id.to_string(),
            });
        }
    }
    Ok(())
}

fn migration_checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

fn render_migration_sql(template: &str, database: &str) -> Result<String, SchemaError> {
    ensure_safe_database(database)?;
    let rendered = template.replace("{{database}}", database);
    if rendered.contains("{{database}}") {
        return Err(SchemaError::UnresolvedDatabasePlaceholder);
    }
    Ok(rendered)
}

fn ensure_safe_database(database: &str) -> Result<(), SchemaError> {
    let safe = !database.is_empty()
        && database
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric());
    safe.then_some(()).ok_or(SchemaError::UnsafeDatabaseName)
}

async fn validate_schema_contract(client: &Client, database: &str) -> Result<(), SchemaError> {
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
            .bind(required.name)
            .fetch_all::<StoredColumn>()
            .await
            .map_err(|source| SchemaError::ClickHouse {
                operation: "inspect schema columns",
                source,
            })?;
        if columns.is_empty() {
            return Err(SchemaError::MissingTable {
                table: required.name.to_string(),
            });
        }
        let stored = columns
            .into_iter()
            .map(|column| (column.name, column.r#type))
            .collect::<HashMap<_, _>>();
        for &(column, expected_type) in required.columns {
            let Some(actual_type) = stored.get(column) else {
                return Err(SchemaError::MissingColumn {
                    table: required.name.to_string(),
                    column: column.to_string(),
                });
            };
            if actual_type != expected_type {
                return Err(SchemaError::ColumnTypeMismatch {
                    table: required.name.to_string(),
                    column: column.to_string(),
                    actual: actual_type.clone(),
                    expected: expected_type.to_string(),
                });
            }
        }
    }

    let views = client
        .query(
            r#"
            SELECT name
            FROM system.tables
            WHERE database = ? AND engine = 'View'
            "#,
        )
        .bind(database)
        .fetch_all::<StoredObject>()
        .await
        .map_err(|source| SchemaError::ClickHouse {
            operation: "inspect schema views",
            source,
        })?;
    let views = views
        .into_iter()
        .map(|item| item.name)
        .collect::<HashSet<_>>();
    for required in REQUIRED_VIEWS {
        if !views.contains(*required) {
            return Err(SchemaError::MissingView {
                view: (*required).to_string(),
            });
        }
    }
    Ok(())
}

struct RequiredTable {
    name: &'static str,
    columns: &'static [(&'static str, &'static str)],
}

const REQUIRED_VIEWS: &[&str] = &[
    "ingested_blocks_canonical",
    "transactions_canonical",
    "evm_logs_canonical",
    "address_relationships_canonical",
    "transaction_features_canonical",
    "semantic_aml_events_canonical",
    "protocol_contract_registry_current",
    "protocol_contract_registry_active",
    "token_metadata_discoveries_canonical",
    "token_metadata_current",
    "token_metadata_jobs_current",
    "sync_state_current",
    "ingestion_failures_current",
];

fn required_tables() -> &'static [RequiredTable] {
    &[
        RequiredTable {
            name: "schema_migrations",
            columns: &[
                ("migration_id", "String"),
                ("description", "String"),
                ("checksum", "String"),
                ("applied_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "ingested_blocks",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("block_number", "UInt64"),
                ("block_hash", "String"),
                ("parent_hash", "String"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("transaction_count", "UInt32"),
                ("log_count", "UInt32"),
                ("receipt_data_complete", "UInt8"),
                ("trace_data_complete", "UInt8"),
                ("canonical", "UInt8"),
                ("ingestion_status", "LowCardinality(String)"),
                ("rpc_provider", "LowCardinality(String)"),
                ("rpc_client_version", "String"),
                ("state_revision", "UInt64"),
                ("indexed_at_unix_ms", "UInt64"),
                ("updated_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "transactions",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("tx_hash", "String"),
                ("block_hash", "String"),
                ("block_number", "UInt64"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("block_state_revision", "UInt64"),
                ("transaction_index", "UInt32"),
                ("from_address", "String"),
                ("to_address", "String"),
                ("contract_address", "String"),
                ("nonce", "UInt64"),
                ("transaction_type", "UInt8"),
                ("value", "UInt256"),
                ("input_selector", "String"),
                ("input_data", "String"),
                ("status", "UInt8"),
                ("gas_limit", "UInt64"),
                ("gas_used", "UInt64"),
                ("effective_gas_price", "UInt256"),
                ("fee_paid", "UInt256"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "evm_logs",
            columns: &[
                ("event_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("block_hash", "String"),
                ("block_number", "UInt64"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("block_state_revision", "UInt64"),
                ("tx_hash", "String"),
                ("transaction_index", "UInt32"),
                ("log_index", "UInt32"),
                ("contract_address", "String"),
                ("topic0", "String"),
                ("topics", "Array(String)"),
                ("data", "String"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "address_relationships",
            columns: &[
                ("relationship_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("block_hash", "String"),
                ("block_number", "UInt64"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("block_state_revision", "UInt64"),
                ("tx_hash", "String"),
                ("transaction_index", "UInt32"),
                ("event_index", "UInt32"),
                ("event_sub_index", "UInt32"),
                ("trace_address", "Array(UInt32)"),
                ("from_address", "String"),
                ("to_address", "String"),
                ("asset_id", "String"),
                ("token_id", "String"),
                ("amount", "UInt256"),
                ("transfer_type", "LowCardinality(String)"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "transaction_features",
            columns: &[
                ("feature_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("block_hash", "String"),
                ("block_number", "UInt64"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("block_state_revision", "UInt64"),
                ("tx_hash", "String"),
                ("transaction_type", "LowCardinality(String)"),
                ("transaction_subtype", "LowCardinality(String)"),
                ("protocol", "String"),
                ("method_id", "String"),
                ("is_swap", "UInt8"),
                ("is_bridge", "UInt8"),
                ("is_mint", "UInt8"),
                ("is_burn", "UInt8"),
                ("is_liquidity_add", "UInt8"),
                ("is_liquidity_remove", "UInt8"),
                ("is_contract_call", "UInt8"),
                ("unique_assets", "UInt16"),
                ("participants", "UInt16"),
                ("classification_confidence", "Float32"),
                ("classification_source", "LowCardinality(String)"),
                ("detector", "String"),
                ("detector_version", "String"),
                ("evidence_refs", "Array(String)"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "semantic_aml_events",
            columns: &[
                ("event_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("block_hash", "String"),
                ("block_number", "UInt64"),
                ("block_timestamp_unix_ms", "UInt64"),
                ("block_state_revision", "UInt64"),
                ("tx_hash", "String"),
                ("event_index", "UInt32"),
                ("event_type", "LowCardinality(String)"),
                ("subject_address", "String"),
                ("protocol", "String"),
                ("protocol_contract", "String"),
                ("counterparty_address", "String"),
                ("correlation_key", "String"),
                ("asset_in", "String"),
                ("asset_out", "String"),
                ("remote_network_id", "String"),
                ("remote_asset", "String"),
                ("bridge_direction", "LowCardinality(String)"),
                ("remote_receiver", "String"),
                ("bridge_message_id", "String"),
                ("amount_in", "String"),
                ("amount_out", "String"),
                ("detector", "String"),
                ("detector_version", "String"),
                ("confidence", "Float32"),
                ("evidence_refs", "Array(String)"),
                ("evidence_json", "String"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "protocol_contract_registry",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("contract_address", "String"),
                ("protocol", "String"),
                ("protocol_type", "LowCardinality(String)"),
                ("contract_role", "LowCardinality(String)"),
                ("decoder", "LowCardinality(String)"),
                ("remote_network_id", "String"),
                ("remote_contract_address", "String"),
                ("method_ids", "Array(String)"),
                ("method_event_types", "Array(String)"),
                ("event_topics", "Array(String)"),
                ("event_types", "Array(String)"),
                ("remote_receiver_topic_index", "Int8"),
                ("message_topic_index", "Int8"),
                ("source_id", "String"),
                ("source_reference", "String"),
                ("review_status", "LowCardinality(String)"),
                ("evidence_confidence", "Float32"),
                ("enabled", "UInt8"),
                ("registry_revision", "UInt64"),
                ("reviewed_by", "String"),
                ("review_note", "String"),
                ("created_at_unix_ms", "UInt64"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "token_metadata",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("token_address", "String"),
                ("token_standard", "LowCardinality(String)"),
                ("name", "String"),
                ("symbol", "String"),
                ("decimals", "Nullable(UInt8)"),
                ("metadata_status", "LowCardinality(String)"),
                ("metadata_source", "LowCardinality(String)"),
                ("is_verified", "UInt8"),
                ("observed_block", "UInt64"),
                ("created_at_unix_ms", "UInt64"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "token_metadata_discoveries",
            columns: &[
                ("discovery_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("token_address", "String"),
                ("standard_hint", "LowCardinality(String)"),
                ("discovered_block", "UInt64"),
                ("block_hash", "String"),
                ("block_state_revision", "UInt64"),
                ("tx_hash", "String"),
                ("evidence_id", "String"),
                ("created_at_unix_ms", "UInt64"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "token_metadata_jobs",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("token_address", "String"),
                ("status", "LowCardinality(String)"),
                ("attempt_count", "UInt16"),
                ("last_error_class", "LowCardinality(String)"),
                ("next_attempt_at_unix_ms", "UInt64"),
                ("updated_at_unix_ms", "UInt64"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "sync_state",
            columns: &[
                ("network_id", "LowCardinality(String)"),
                ("next_block", "UInt64"),
                ("last_finalized_block", "UInt64"),
                ("last_finalized_block_hash", "String"),
                ("state_revision", "UInt64"),
                ("updated_at_unix_ms", "UInt64"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "ingestion_failures",
            columns: &[
                ("failure_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("block_number", "UInt64"),
                ("block_hash", "String"),
                ("stage", "LowCardinality(String)"),
                ("error_class", "LowCardinality(String)"),
                ("error_summary", "String"),
                ("retryable", "UInt8"),
                ("attempt_count", "UInt32"),
                ("status", "LowCardinality(String)"),
                ("created_at_unix_ms", "UInt64"),
                ("updated_at_unix_ms", "UInt64"),
                ("resolved_at_unix_ms", "UInt64"),
                ("inserted_at", "DateTime64(3)"),
            ],
        },
        RequiredTable {
            name: "ingestion_benchmarks",
            columns: &[
                ("benchmark_id", "String"),
                ("network_id", "LowCardinality(String)"),
                ("start_block", "UInt64"),
                ("end_block", "UInt64"),
                ("completed_blocks", "UInt64"),
                ("transaction_count", "UInt64"),
                ("log_count", "UInt64"),
                ("relationship_count", "UInt64"),
                ("feature_count", "UInt64"),
                ("semantic_event_count", "UInt64"),
                ("elapsed_ms", "UInt64"),
                ("blocks_per_second", "Float64"),
                ("observed_live_blocks_per_second", "Float64"),
                ("live_rate_multiple", "Float64"),
                ("rows_per_second", "Float64"),
                ("compressed_bytes", "UInt64"),
                ("uncompressed_bytes", "UInt64"),
                ("created_at", "DateTime64(3)"),
            ],
        },
    ]
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("unsafe ClickHouse database name")]
    UnsafeDatabaseName,
    #[error("migration contains an unresolved database placeholder")]
    UnresolvedDatabasePlaceholder,
    #[error("duplicate migration id {migration_id}")]
    DuplicateMigrationId { migration_id: String },
    #[error("required migration {migration_id} is missing")]
    MissingMigration { migration_id: String },
    #[error("migration {migration_id} checksum changed; create a new migration")]
    ChecksumChanged { migration_id: String },
    #[error("migration {migration_id} failed at statement {statement_index}")]
    MigrationSql {
        migration_id: String,
        statement_index: usize,
        #[source]
        source: clickhouse::error::Error,
    },
    #[error("ClickHouse operation failed: {operation}")]
    ClickHouse {
        operation: &'static str,
        #[source]
        source: clickhouse::error::Error,
    },
    #[error("required table {table} is missing")]
    MissingTable { table: String },
    #[error("required view {view} is missing")]
    MissingView { view: String },
    #[error("required column {table}.{column} is missing")]
    MissingColumn { table: String, column: String },
    #[error("column {table}.{column} has type {actual}, expected {expected}")]
    ColumnTypeMismatch {
        table: String,
        column: String,
        actual: String,
        expected: String,
    },
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    use clickhouse::Client;
    use serde::Deserialize;

    use super::{
        MIGRATIONS, Migration, SchemaError, ensure_safe_database, initialize_database,
        migration_checksum, render_migration_sql, validate_migration_manifest,
    };

    #[test]
    fn migration_ids_and_checksums_are_unique() {
        validate_migration_manifest(MIGRATIONS).unwrap();
        let checksums = MIGRATIONS
            .iter()
            .map(|migration| migration_checksum(migration.sql_template))
            .collect::<HashSet<_>>();
        assert_eq!(checksums.len(), MIGRATIONS.len());
    }

    #[test]
    fn renders_only_safe_database_identifiers() {
        let sql =
            render_migration_sql("CREATE TABLE {{database}}.x (id UInt8)", "bsc_aml").unwrap();
        assert_eq!(sql, "CREATE TABLE bsc_aml.x (id UInt8)");
        assert!(ensure_safe_database("bsc_aml; DROP DATABASE bsc_aml").is_err());
    }

    #[derive(Deserialize, clickhouse::Row)]
    struct CountRow {
        count: u64,
    }

    #[derive(Deserialize, clickhouse::Row)]
    struct HashRow {
        tx_hash: String,
    }

    #[tokio::test]
    #[ignore = "requires disposable ClickHouse configured through BSC_TEST_CLICKHOUSE_* settings"]
    async fn clickhouse_schema_lifecycle_and_canonicalization() {
        let url = env::var("BSC_TEST_CLICKHOUSE_URL")
            .expect("BSC_TEST_CLICKHOUSE_URL must target a disposable ClickHouse instance");
        let user = env::var("BSC_TEST_CLICKHOUSE_USER").unwrap_or_else(|_| "bsc_admin".into());
        let password = env::var("BSC_TEST_CLICKHOUSE_PASSWORD")
            .expect("BSC_TEST_CLICKHOUSE_PASSWORD is required");
        let client = Client::default()
            .with_url(&url)
            .with_user(&user)
            .with_password(&password);
        let suffix = format!(
            "{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let database = format!("bsc_aml_it_{suffix}");
        let forward_database = format!("bsc_aml_forward_it_{suffix}");

        let first = initialize_database(&client, &database, MIGRATIONS)
            .await
            .unwrap();
        assert_eq!(first.newly_applied, MIGRATIONS.len());
        let restart = initialize_database(&client, &database, MIGRATIONS)
            .await
            .unwrap();
        assert_eq!(restart.newly_applied, 0);

        client
            .query(&format!(
                r#"
                INSERT INTO {database}.ingested_blocks
                    (network_id, block_number, block_hash, parent_hash,
                     block_timestamp_unix_ms, receipt_data_complete, trace_data_complete,
                     canonical, ingestion_status, state_revision, indexed_at_unix_ms)
                VALUES ('eip155:56', 7, '0xold', '0xparent', 1700000000000,
                        1, 1, 1, 'complete', 1, 1700000001000)
                "#
            ))
            .execute()
            .await
            .unwrap();
        for _ in 0..2 {
            client
                .query(&format!(
                    r#"
                    INSERT INTO {database}.transactions
                        (network_id, tx_hash, block_hash, block_number,
                         block_timestamp_unix_ms, block_state_revision, transaction_index)
                    VALUES ('eip155:56', '0xoldtx', '0xold', 7, 1700000000000, 1, 0)
                    "#
                ))
                .execute()
                .await
                .unwrap();
        }
        let duplicate_count = client
            .query(&format!(
                "SELECT count() AS count FROM {database}.transactions_canonical"
            ))
            .fetch_one::<CountRow>()
            .await
            .unwrap();
        assert_eq!(duplicate_count.count, 1);

        client
            .query(&format!(
                r#"
                INSERT INTO {database}.ingested_blocks
                    (network_id, block_number, block_hash, parent_hash,
                     block_timestamp_unix_ms, receipt_data_complete, trace_data_complete,
                     canonical, ingestion_status, state_revision, indexed_at_unix_ms)
                VALUES ('eip155:56', 7, '0xnew', '0xparent', 1700000000000,
                        1, 1, 1, 'complete', 2, 1700000002000)
                "#
            ))
            .execute()
            .await
            .unwrap();
        client
            .query(&format!(
                r#"
                INSERT INTO {database}.transactions
                    (network_id, tx_hash, block_hash, block_number,
                     block_timestamp_unix_ms, block_state_revision, transaction_index)
                VALUES ('eip155:56', '0xnewtx', '0xnew', 7, 1700000000000, 2, 0)
                "#
            ))
            .execute()
            .await
            .unwrap();
        let canonical = client
            .query(&format!(
                "SELECT tx_hash FROM {database}.transactions_canonical"
            ))
            .fetch_all::<HashRow>()
            .await
            .unwrap();
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].tx_hash, "0xnewtx");

        const FORWARD_ONE: Migration = Migration {
            id: "test_0001",
            description: "test base",
            sql_template: "CREATE TABLE IF NOT EXISTS {{database}}.forward_one (id UInt8) ENGINE = MergeTree ORDER BY id;",
        };
        const FORWARD_TWO: Migration = Migration {
            id: "test_0002",
            description: "test forward",
            sql_template: "CREATE TABLE IF NOT EXISTS {{database}}.forward_two (id UInt8) ENGINE = MergeTree ORDER BY id;",
        };
        let base = initialize_database(&client, &forward_database, &[FORWARD_ONE])
            .await
            .unwrap();
        assert_eq!(base.newly_applied, 1);
        let forward = initialize_database(&client, &forward_database, &[FORWARD_ONE, FORWARD_TWO])
            .await
            .unwrap();
        assert_eq!(forward.newly_applied, 1);

        const CHANGED_ONE: Migration = Migration {
            id: "test_0001",
            description: "illegally changed test base",
            sql_template: "CREATE TABLE IF NOT EXISTS {{database}}.forward_one (id UInt16) ENGINE = MergeTree ORDER BY id;",
        };
        assert!(matches!(
            initialize_database(&client, &forward_database, &[CHANGED_ONE]).await,
            Err(SchemaError::ChecksumChanged { .. })
        ));

        for disposable in [&database, &forward_database] {
            client
                .query(&format!("DROP DATABASE IF EXISTS {disposable}"))
                .execute()
                .await
                .unwrap();
        }
    }
}
