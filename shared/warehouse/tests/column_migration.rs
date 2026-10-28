//! Only a new disposable ClickHouse fixture; never reads deployment credentials.
use aml_warehouse::{Connection, Network, baseline_contract, initialize, validate};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
mod support;
use support::{fixture, historical_fixture};

struct Http {
    url: String,
    client: reqwest::Client,
}
impl Http {
    fn new() -> Result<Self> {
        Ok(Self {
            url: std::env::var("WAREHOUSE_TEST_URL")?,
            client: reqwest::Client::new(),
        })
    }
    async fn request(&self, sql: &str) -> Result<String> {
        let response = self
            .client
            .post(&self.url)
            .header("X-ClickHouse-User", "warehouse_test")
            .header("X-ClickHouse-Key", "warehouse-test-only")
            .body(sql.to_owned())
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        ensure!(
            status.is_success(),
            "ClickHouse rejected fixture query: {body}"
        );
        Ok(body)
    }
    async fn scalar(&self, sql: &str) -> Result<String> {
        self.query(sql).await?.pop().context("empty scalar")
    }
    async fn insert(&self, database: &str, table: &str, row: &Value) -> Result<()> {
        self.execute(&format!(
            "INSERT INTO {database}.{table} FORMAT JSONEachRow\n{}",
            serde_json::to_string(row)?
        ))
        .await
    }
}
impl Connection for Http {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.request(sql).await?;
        Ok(())
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.request(&format!("{sql} FORMAT JSONEachRow"))
            .await?
            .lines()
            .map(|line| {
                let row: Value = serde_json::from_str(line)?;
                row.as_object()
                    .and_then(|o| o.values().next())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("expected String column")
            })
            .collect()
    }
}
async fn baseline(db: &Http, database: &str, network: Network) -> Result<()> {
    ensure!(
        db.scalar(&format!(
            "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
        ))
        .await?
            == "0",
        "requires a new disposable fixture database"
    );
    db.execute(&format!("CREATE DATABASE {database}")).await?;
    let contract = baseline_contract()?;
    for statement in contract.statements(database)? {
        db.execute(&statement).await?;
    }
    let mut ledger = fixture("aml_schema_migrations", network)?;
    ledger["migration_id"] = json!(contract.migration_id);
    ledger["checksum"] = json!(contract.checksum()?);
    ledger["schema_version"] = json!(1);
    db.insert(database, "aml_schema_migrations", &ledger)
        .await?;
    let mut identity = fixture("aml_warehouse_identity", network)?;
    identity["checksum"] = json!(contract.checksum()?);
    identity["schema_version"] = json!(1);
    // Simulates clock rollback: a new identity must still supersede this row.
    identity["revision"] = json!(9_000_000_000_000_000_000u64);
    db.insert(database, "aml_warehouse_identity", &identity)
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn v1_upgrade_preserves_every_table_and_enforces_identical_column_rules_on_four_networks()
-> Result<()> {
    let db = Http::new()?;
    let contract = baseline_contract()?;
    let mut shapes = Vec::new();
    for network in Network::ALL {
        let database = format!("db03_upgrade_{}", network.name());
        baseline(&db, &database, network).await?;
        let mut before = Vec::new();
        for table in &contract.tables {
            if matches!(
                table.name.as_str(),
                "aml_schema_migrations" | "aml_warehouse_identity"
            ) {
                continue;
            }
            let row = historical_fixture(&table.name, network)?;
            if aml_warehouse::semantics::validate_row(
                &aml_warehouse::column_contract()?,
                &table.name,
                &row,
            )
            .is_err()
            {
                continue;
            }
            db.insert(&database, &table.name, &row).await?;
            before.push((
                table.name.clone(),
                db.request(&format!(
                    "SELECT * FROM {database}.{} FINAL FORMAT JSONEachRow",
                    table.name
                ))
                .await?,
            ));
        }
        assert!(
            validate(&db, &database, network).await.is_err(),
            "v1 requires the schema job, not a silent read upgrade"
        );
        let summary = initialize(&db, &database, network).await?;
        assert_eq!(summary.schema_version, 5);
        assert!(summary.newly_applied);
        assert!(!initialize(&db, &database, network).await?.newly_applied);
        validate(&db, &database, network).await?;
        for (table, rows) in before {
            let baseline = baseline_contract()?;
            let columns = baseline
                .tables
                .iter()
                .find(|t| t.name == table)
                .unwrap()
                .columns
                .iter()
                .map(|c| format!("`{}`", c.name))
                .collect::<Vec<_>>()
                .join(",");
            assert_eq!(
                db.request(&format!(
                    "SELECT {columns} FROM {database}.{table} FINAL FORMAT JSONEachRow"
                ))
                .await?,
                rows,
                "historical {table} changed"
            );
        }
        assert_eq!(
            db.scalar(&format!(
                "SELECT toString(count()) FROM {database}.aml_schema_migrations FINAL"
            ))
            .await?,
            "4"
        );
        assert_eq!(
            db.scalar(&format!(
                "SELECT toString(schema_version) FROM {database}.aml_warehouse_identity FINAL"
            ))
            .await?,
            "4"
        );
        let shape=db.query(&format!("SELECT replaceAll(create_table_query,'{database}.','warehouse.') FROM system.tables WHERE database='{database}' AND engine!='View' ORDER BY name")).await?;
        shapes.push(shape);
        for (table, field, value) in [
            ("aml_address_relationships", "confidence", json!(1.01)),
            (
                "aml_intelligence_claims",
                "review_status",
                json!("verified"),
            ),
            (
                "aml_transactions",
                "decimals",
                json!(network.native_decimals() + 1),
            ),
            ("aml_transactions", "block_hash", json!("A".repeat(64))),
            (
                "aml_transactions",
                "raw_evidence_json",
                json!("invalid-json"),
            ),
            (
                "aml_address_relationships",
                "evidence_refs",
                json!(["unqualified"]),
            ),
        ] {
            let mut row = fixture(table, network)?;
            row[field] = value;
            assert!(
                db.insert(&database, table, &row).await.is_err(),
                "ClickHouse accepted {table}.{field}"
            );
        }
        let mut metadata = fixture("aml_asset_metadata", network)?;
        metadata["decimals"] = Value::Null;
        assert!(
            db.insert(&database, "aml_asset_metadata", &metadata)
                .await
                .is_err(),
            "native decimals cannot be unknown"
        );
        let mut review = fixture("aml_intelligence_claims", network)?;
        review["review_status"] = json!("approved");
        review["reviewed_by"] = json!("");
        assert!(
            db.insert(&database, "aml_intelligence_claims", &review)
                .await
                .is_err()
        );
        let mut refs = fixture("aml_address_relationships", network)?;
        refs["evidence_refs"] = json!([format!(
            "tx:{}",
            aml_warehouse::values::normalize_hash(network, &"b".repeat(64))?
        )]);
        db.insert(&database, "aml_address_relationships", &refs)
            .await?;
        if network == Network::Bitcoin {
            let mut row = fixture("aml_transactions", network)?;
            row["amount_raw"] = json!("1");
            assert!(
                db.insert(&database, "aml_transactions", &row)
                    .await
                    .is_err()
            );
            let mut row = fixture("aml_ingested_blocks", network)?;
            row["receipt_coverage"] = json!(0);
            assert!(
                db.insert(&database, "aml_ingested_blocks", &row)
                    .await
                    .is_err()
            );
            for index in ["4294967296", "18446744073709551616", "00"] {
                let mut row = fixture("aml_address_relationships", network)?;
                row["evidence_refs"] = json!([format!("outpoint:{}:{index}", "a".repeat(64))]);
                assert!(
                    db.insert(&database, "aml_address_relationships", &row)
                        .await
                        .is_err()
                );
            }
        }
    }
    assert!(
        shapes.windows(2).all(|pair| pair[0] == pair[1]),
        "column constraints differ between network databases"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn incompatible_v1_history_is_rejected_before_any_upgrade_ddl_without_rewriting_evidence()
-> Result<()> {
    let db = Http::new()?;
    let database = "db03_invalid_history";
    let network = Network::Ethereum;
    baseline(&db, database, network).await?;
    let mut row = fixture("aml_intelligence_claims", network)?;
    row["confidence"] = json!(1.5);
    db.insert(database, "aml_intelligence_claims", &row).await?;
    let before = db
        .request(&format!(
            "SELECT * FROM {database}.aml_intelligence_claims FINAL FORMAT JSONEachRow"
        ))
        .await?;
    assert!(initialize(&db, database, network).await.is_err());
    assert_eq!(db.scalar(&format!("SELECT toString(count()) FROM system.tables WHERE database='{database}' AND position(create_table_query,'CONSTRAINT ')>0")).await?,"0");
    assert_eq!(
        db.scalar(&format!(
            "SELECT toString(count()) FROM {database}.aml_schema_migrations FINAL"
        ))
        .await?,
        "1"
    );
    assert_eq!(
        db.request(&format!(
            "SELECT * FROM {database}.aml_intelligence_claims FINAL FORMAT JSONEachRow"
        ))
        .await?,
        before
    );
    let missing = "db03_missing_history_table";
    baseline(&db, missing, network).await?;
    db.execute(&format!("DROP TABLE {missing}.aml_ingestion_benchmarks"))
        .await?;
    assert!(initialize(&db, missing, network).await.is_err());
    assert_eq!(db.scalar(&format!("SELECT toString(count()) FROM system.tables WHERE database='{missing}' AND name='aml_ingestion_benchmarks'")).await?,"0");
    let exhausted = "db03_exhausted_identity";
    baseline(&db, exhausted, network).await?;
    let mut identity = fixture("aml_warehouse_identity", network)?;
    identity["schema_version"] = json!(1);
    identity["checksum"] = json!(baseline_contract()?.checksum()?);
    identity["revision"] = json!(u64::MAX);
    db.insert(exhausted, "aml_warehouse_identity", &identity)
        .await?;
    assert!(initialize(&db, exhausted, network).await.is_err());
    assert_eq!(db.scalar(&format!("SELECT toString(count()) FROM system.tables WHERE database='{exhausted}' AND position(create_table_query,'CONSTRAINT ')>0")).await?,"0");
    let near = "db04_near_exhausted_identity";
    baseline(&db, near, network).await?;
    identity["revision"] = json!(u64::MAX - 1);
    db.insert(near, "aml_warehouse_identity", &identity).await?;
    assert!(
        initialize(&db, near, network).await.is_err(),
        "both remaining identity revisions must be reserved before DDL"
    );
    assert_eq!(db.scalar(&format!("SELECT toString(count()) FROM system.tables WHERE database='{near}' AND position(create_table_query,'CONSTRAINT ')>0")).await?,"0");
    Ok(())
}

struct FailUpgrade<'a> {
    db: &'a Http,
    remaining: AtomicUsize,
}

async fn version_two(db: &Http, database: &str, network: Network) -> Result<()> {
    baseline(db, database, network).await?;
    for statement in aml_warehouse::semantics::upgrade_statements(
        database,
        &aml_warehouse::semantics::upgrade_template()?,
    )? {
        db.execute(&statement).await?;
    }
    let contract = aml_warehouse::column_contract()?;
    let mut ledger = historical_fixture("aml_schema_migrations", network)?;
    ledger["migration_id"] = json!(contract.migration_id);
    ledger["checksum"] = json!(contract.checksum()?);
    ledger["schema_version"] = json!(2);
    db.insert(database, "aml_schema_migrations", &ledger)
        .await?;
    let mut identity = historical_fixture("aml_warehouse_identity", network)?;
    identity["checksum"] = json!(contract.checksum()?);
    identity["schema_version"] = json!(2);
    identity["revision"] = json!(9_000_000_000_000_000_001u64);
    db.insert(database, "aml_warehouse_identity", &identity)
        .await
}
struct FailCore<'a> {
    db: &'a Http,
    prefix: &'a str,
    remaining: AtomicUsize,
}
impl Connection for FailCore<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        if sql.starts_with(self.prefix) {
            ensure!(
                self.remaining.fetch_sub(1, Ordering::SeqCst) > 1,
                "simulated core migration interruption"
            );
        }
        self.db.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.db.query(sql).await
    }
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn v2_core_upgrade_preserves_original_columns_and_unknown_lifecycle_on_four_networks()
-> Result<()> {
    let db = Http::new()?;
    for network in Network::ALL {
        let database = format!("db04_v2_{}", network.name());
        version_two(&db, &database, network).await?;
        let contract = aml_warehouse::column_contract()?;
        let mut before = Vec::new();
        for table in &contract.tables {
            if !aml_warehouse::core_schema::migration()?
                .columns
                .contains_key(&table.name)
            {
                continue;
            }
            let row = historical_fixture(&table.name, network)?;
            db.insert(&database, &table.name, &row).await?;
            let columns = table
                .columns
                .iter()
                .map(|c| format!("`{}`", c.name))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT {columns} FROM {database}.{} FINAL FORMAT JSONEachRow",
                table.name
            );
            before.push((sql.clone(), db.request(&sql).await?));
        }
        assert_eq!(initialize(&db, &database, network).await?.schema_version, 5);
        for (sql, rows) in before {
            assert_eq!(db.request(&sql).await?, rows, "v2 source values changed");
        }
        assert_eq!(db.scalar(&format!("SELECT toString(count()) FROM {database}.aml_ingestion_failures WHERE isNull(retryable) AND isNull(tx_hash) AND isNull(first_failed_at_unix_ms) AND isNull(last_failed_at_unix_ms) AND isNull(resolved_at_unix_ms) AND raw_evidence_json='{{}}'")).await?,"1");
        assert_eq!(
            db.scalar(&format!(
                "SELECT status FROM {database}.aml_ingestion_benchmarks_current"
            ))
            .await?,
            "unknown"
        );
        validate(&db, &database, network).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn interrupted_core_columns_constraints_and_views_resume_but_drift_is_rejected() -> Result<()>
{
    let db = Http::new()?;
    let network = Network::Bitcoin;
    for (case, prefix, remaining) in [
        ("columns", "ALTER TABLE", 6),
        ("constraints", "ALTER TABLE", 32),
        ("views", "CREATE OR REPLACE VIEW", 4),
    ] {
        let database = format!("db04_partial_{case}");
        version_two(&db, &database, network).await?;
        let before = db
            .scalar(&format!(
                "SELECT toString(count()) FROM {database}.aml_schema_migrations FINAL"
            ))
            .await?;
        assert!(
            initialize(
                &FailCore {
                    db: &db,
                    prefix,
                    remaining: AtomicUsize::new(remaining)
                },
                &database,
                network
            )
            .await
            .is_err()
        );
        assert_eq!(
            db.scalar(&format!(
                "SELECT toString(count()) FROM {database}.aml_schema_migrations FINAL"
            ))
            .await?,
            before
        );
        initialize(&db, &database, network).await?;
        validate(&db, &database, network).await?;
        db.execute(&format!(
            "ALTER TABLE {database}.aml_address_relationships DROP CONSTRAINT aml_core_v3"
        ))
        .await?;
        assert!(
            initialize(&db, &database, network).await.is_err(),
            "applied missing constraint cannot be repaired silently"
        );
    }
    for (case, ddl) in [
        ("wrong_type", "ADD COLUMN event_sub_index UInt64 DEFAULT 0"),
        (
            "wrong_default",
            "ADD COLUMN event_sub_index UInt32 DEFAULT 7",
        ),
        ("unknown_column", "ADD COLUMN unexpected_source String"),
        ("wrong_view", "MODIFY COMMENT 'unused'"),
    ] {
        let database = format!("db04_drift_{case}");
        version_two(&db, &database, network).await?;
        if case == "wrong_view" {
            db.execute(&format!("CREATE OR REPLACE VIEW {database}.aml_ingested_blocks_canonical AS SELECT * FROM {database}.aml_ingested_blocks WHERE canonical=1")).await?;
        } else {
            db.execute(&format!(
                "ALTER TABLE {database}.aml_address_relationships {ddl}"
            ))
            .await?;
        }
        assert!(
            initialize(&db, &database, network).await.is_err(),
            "unexpected {case} was accepted"
        );
        assert_eq!(
            db.scalar(&format!(
                "SELECT toString(count()) FROM {database}.aml_schema_migrations FINAL"
            ))
            .await?,
            "2"
        );
    }
    Ok(())
}
impl Connection for FailUpgrade<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        if sql.starts_with("ALTER TABLE") {
            ensure!(
                self.remaining.fetch_sub(1, Ordering::SeqCst) > 1,
                "simulated upgrade crash"
            );
        }
        self.db.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.db.query(sql).await
    }
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn partial_column_upgrade_resumes_but_applied_constraint_drift_is_never_blessed() -> Result<()>
{
    let db = Http::new()?;
    let database = "db03_interrupted";
    let network = Network::Bitcoin;
    baseline(&db, database, network).await?;
    assert!(
        initialize(
            &FailUpgrade {
                db: &db,
                remaining: AtomicUsize::new(6)
            },
            database,
            network
        )
        .await
        .is_err()
    );
    assert_eq!(db.scalar(&format!("SELECT toString(count()) FROM system.tables WHERE database='{database}' AND position(create_table_query,'CONSTRAINT ')>0")).await?,"5");
    initialize(&db, database, network).await?;
    db.execute(&format!(
        "ALTER TABLE {database}.aml_transactions DROP CONSTRAINT aml_columns_v2"
    ))
    .await?;
    assert!(validate(&db, database, network).await.is_err());
    assert!(initialize(&db, database, network).await.is_err());
    db.execute(&format!(
        "ALTER TABLE {database}.aml_transactions ADD CONSTRAINT aml_columns_v2 CHECK 1"
    ))
    .await?;
    assert!(validate(&db, database, network).await.is_err());
    assert!(initialize(&db, database, network).await.is_err());
    Ok(())
}
