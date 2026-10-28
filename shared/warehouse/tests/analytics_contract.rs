//! DB-05 uses empty fixture databases and fixture credentials exclusively.
use aml_warehouse::{
    Connection, Network,
    analytics::{self, Mode},
    core, initialize, publication, validate,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};
mod support;
use support::fixture;
struct Http {
    client: reqwest::Client,
    url: String,
}
impl Http {
    fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::new(),
            url: std::env::var("WAREHOUSE_TEST_URL")?,
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
        ensure!(status.is_success(), "ClickHouse: {body}");
        Ok(body)
    }
    async fn row(&self, database: &str, table: &str, row: &Value) -> Result<()> {
        self.execute(&format!(
            "INSERT INTO {database}.{table} FORMAT JSONEachRow\n{row}"
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
                let v: Value = serde_json::from_str(line)?;
                v.as_object()
                    .and_then(|o| o.values().next())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("expected String result")
            })
            .collect()
    }
}
async fn version_three(db: &Http, database: &str, network: Network) -> Result<()> {
    ensure!(
        db.query(&format!(
            "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
        ))
        .await?
            == ["0"],
        "fixture must be empty"
    );
    db.execute(&format!("CREATE DATABASE {database}")).await?;
    for statement in aml_warehouse::core_contract()?.statements(database)? {
        db.execute(&statement).await?;
    }
    for contract in [
        aml_warehouse::baseline_contract()?,
        aml_warehouse::column_contract()?,
        aml_warehouse::core_contract()?,
    ] {
        let mut row = fixture("aml_schema_migrations", network)?;
        row["migration_id"] = json!(contract.migration_id);
        row["checksum"] = json!(contract.checksum()?);
        row["schema_version"] = json!(contract.version);
        db.row(database, "aml_schema_migrations", &row).await?;
    }
    let mut identity = fixture("aml_warehouse_identity", network)?;
    identity["schema_version"] = json!(3);
    identity["checksum"] = json!(aml_warehouse::core_contract()?.checksum()?);
    identity["revision"] = json!(9_000_000_000_000_000_000u64);
    db.row(database, "aml_warehouse_identity", &identity).await
}
#[test]
fn immutable_predecessors_and_complete_analytics_columns_are_preserved() -> Result<()> {
    assert_eq!(
        aml_warehouse::core_contract()?.checksum()?,
        "9548e226c0d1d4259fe488bab1d9a137232e6e68d311054190b0034db7e0800e"
    );
    let contract = aml_warehouse::analytics_contract()?;
    assert_eq!(
        (
            contract.version,
            contract.tables.len(),
            contract.views.len()
        ),
        (4, 38, 40)
    );
    assert_eq!(
        contract
            .tables
            .iter()
            .map(|t| t.columns.len())
            .sum::<usize>(),
        668
    );
    let sql = aml_warehouse::analytics_schema::sql_template()?;
    assert!(!sql.contains("DROP ") && !sql.contains("TRUNCATE "));
    assert!(
        aml_warehouse::analytics_schema::statements(
            "fixture",
            &sql.replace("ADD COLUMN", "DROP COLUMN")
        )
        .is_err()
    );
    let mut row = fixture("aml_cluster_claims", Network::Ethereum)?;
    row["left_subject_id"] = json!("left");
    row["right_subject_id"] = json!("right");
    row["same_entity"] = json!(0);
    aml_warehouse::semantics::validate_row(&contract, "aml_cluster_claims", &row)?;
    row["same_entity"] = json!(2);
    assert!(aml_warehouse::semantics::validate_row(&contract, "aml_cluster_claims", &row).is_err());
    let mut raw = fixture("aml_raw_transactions", Network::Bitcoin)?;
    raw["receipt_json"] = json!("{}");
    assert!(
        aml_warehouse::semantics::validate_row(&contract, "aml_raw_transactions", &raw).is_err()
    );
    let mut metadata = fixture("aml_asset_metadata", Network::Bitcoin)?;
    metadata["total_supply_raw"] = json!("2100000000000001");
    assert!(
        aml_warehouse::semantics::validate_row(&contract, "aml_asset_metadata", &metadata).is_err()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn v3_upgrade_preserves_all_analytics_values_on_four_networks() -> Result<()> {
    let db = Http::new()?;
    let old = aml_warehouse::core_contract()?;
    let m = aml_warehouse::analytics_schema::migration()?;
    let mut shapes = vec![];
    for network in Network::ALL {
        let database = format!("db05_upgrade_{}", network.name());
        version_three(&db, &database, network).await?;
        let mut originals = Vec::new();
        for table in old
            .tables
            .iter()
            .filter(|t| m.columns.contains_key(&t.name))
        {
            if table.name == "aml_evm_logs" && !matches!(network, Network::Ethereum | Network::Bsc)
            {
                continue;
            }
            let mut row = fixture(&table.name, network)?;
            row.as_object_mut()
                .unwrap()
                .retain(|key, _| table.columns.iter().any(|c| &c.name == key));
            db.row(&database, &table.name, &row).await?;
            let fields = table
                .columns
                .iter()
                .map(|c| format!("`{}`", c.name))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT {fields} FROM {database}.{} FINAL FORMAT JSONEachRow",
                table.name
            );
            originals.push((sql.clone(), db.request(&sql).await?));
        }
        let result = initialize(&db, &database, network).await?;
        assert_eq!(
            (result.schema_version, result.table_count, result.view_count),
            (5, 39, 53)
        );
        assert!(result.newly_applied);
        assert!(!initialize(&db, &database, network).await?.newly_applied);
        for (sql, before) in originals {
            assert_eq!(db.request(&sql).await?, before);
        }
        validate(&db, &database, network).await?;
        // Exercise every supported family, including empty views: a missing
        // projected column is a reader defect even before any row is written.
        for table in m.columns.keys().chain(m.tables.iter().map(|t| &t.name)) {
            analytics::read(&db, &database, network, table, Mode::Audit, 10).await?;
        }
        let shape=db.query(&format!("SELECT replaceAll(create_table_query,'{database}.','warehouse.') FROM system.tables WHERE database='{database}' ORDER BY name")).await?;
        shapes.push(shape);
        let metadata = analytics::read(
            &db,
            &database,
            network,
            "aml_asset_metadata",
            Mode::Audit,
            10,
        )
        .await?;
        assert!(metadata[0]["is_verified"].is_null());
        assert!(metadata[0]["total_supply_raw"].is_null());
        assert!(
            analytics::read(
                &db,
                &database,
                if network == Network::Bitcoin {
                    Network::Tron
                } else {
                    Network::Bitcoin
                },
                "aml_asset_metadata",
                Mode::Audit,
                10
            )
            .await?
            .is_empty()
        );
    }
    assert!(
        shapes.windows(2).all(|p| p[0] == p[1]),
        "schema differs across independent network databases"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn reviewed_entity_protocol_and_claim_projections_obey_revocation_and_source_activity()
-> Result<()> {
    let db = Http::new()?;
    for network in Network::ALL {
        let database = format!("db05_review_{}", network.name());
        version_three(&db, &database, network).await?;
        initialize(&db, &database, network).await?;
        for table in [
            "aml_intelligence_sources",
            "aml_intelligence_claims",
            "aml_entities",
            "aml_entity_addresses",
            "aml_protocol_registry",
        ] {
            let mut row = fixture(table, network)?;
            if row.get("review_status").is_some() {
                row["review_status"] = json!("approved");
                row["reviewed_by"] = json!("analyst");
            }
            if row.get("is_active").is_some() {
                row["is_active"] = json!(1);
            }
            if row.get("enabled").is_some() {
                row["enabled"] = json!(1);
            }
            if row.get("entity_type").is_some() {
                row["entity_type"] = json!("exchange");
            }
            analytics::insert(&db, &database, table, &[row]).await?;
        }
        for table in [
            "aml_intelligence_claims",
            "aml_entity_addresses",
            "aml_protocol_registry",
        ] {
            assert_eq!(
                analytics::read(&db, &database, network, table, Mode::Approved, 10)
                    .await?
                    .len(),
                1
            );
        }
        assert_eq!(
            db.query(&format!(
                "SELECT toString(count()) FROM {database}.aml_exchange_addresses_active"
            ))
            .await?,
            ["1"]
        );
        let mut claim = fixture("aml_intelligence_claims", network)?;
        claim["revision"] = json!(2);
        claim["review_status"] = json!("revoked");
        claim["reviewed_by"] = json!("reviewer");
        analytics::insert(&db, &database, "aml_intelligence_claims", &[claim.clone()]).await?;
        for table in ["aml_intelligence_claims", "aml_entity_addresses"] {
            assert!(
                analytics::read(&db, &database, network, table, Mode::Approved, 10)
                    .await?
                    .is_empty(),
                "revocation resurrected an old approval"
            );
        }
        claim["revision"] = json!(3);
        claim["review_status"] = json!("approved");
        claim["valid_to_unix_ms"] = json!(1);
        analytics::insert(&db, &database, "aml_intelligence_claims", &[claim]).await?;
        assert!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_intelligence_claims",
                Mode::Approved,
                10
            )
            .await?
            .is_empty()
        );
        let mut source = fixture("aml_intelligence_sources", network)?;
        source["revision"] = json!(2);
        source["is_active"] = json!(0);
        analytics::insert(&db, &database, "aml_intelligence_sources", &[source]).await?;
        assert!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_protocol_registry",
                Mode::Approved,
                10
            )
            .await?
            .is_empty()
        );
        let mut invalid = fixture("aml_protocol_registry", network)?;
        invalid["confidence"] = json!(1.1);
        assert!(
            db.row(&database, "aml_protocol_registry", &invalid)
                .await
                .is_err()
        );
        let mut missing = fixture("aml_entities", network)?;
        missing["review_status"] = json!("approved");
        missing["reviewed_by"] = json!("");
        assert!(db.row(&database, "aml_entities", &missing).await.is_err());
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn specialized_payloads_and_discoveries_share_the_exact_block_fence() -> Result<()> {
    let db = Http::new()?;
    for network in Network::ALL {
        let database = format!("db05_fact_{}", network.name());
        version_three(&db, &database, network).await?;
        initialize(&db, &database, network).await?;
        let mut marker = fixture("aml_ingested_blocks", network)?;
        marker["revision"] = json!(3);
        marker["canonical"] = json!(1);
        marker["ingestion_status"] = json!("complete");
        marker["transaction_count"] = json!(1);
        let block: publication::Block = serde_json::from_value(marker)?;
        let header = serde_json::to_value(&block)?;
        let mut families = vec![
            "aml_transactions",
            "aml_metadata_discoveries",
            "aml_raw_transactions",
        ];
        if network == Network::Bitcoin {
            families.push("aml_bitcoin_scripts");
        }
        if matches!(network, Network::Ethereum | Network::Bsc) {
            families.push("aml_evm_traces");
            families.push("aml_evm_logs");
        }
        let mut facts = BTreeMap::new();
        for table in families {
            let mut row = fixture(table, network)?;
            for field in [
                "network_id",
                "block_number",
                "block_hash",
                "commit_id",
                "revision",
                "block_timestamp_unix_ms",
            ] {
                row[field] = header[field].clone();
            }
            if table == "aml_evm_traces" {
                row["value_raw"] = json!(aml_warehouse::RawAmount::MAX);
                row["trace_address"] = json!([2, 0, 7]);
            }
            if table == "aml_raw_transactions" {
                row["payload_json"] =
                    json!(r#"{"unknown":{"large":"9007199254740993","type":"original ? '"}}"#);
                row["receipt_json"] = Value::Null;
            }
            facts.insert(table.to_owned(), vec![row]);
        }
        let previous = publication::state(&db, &database, network).await?;
        for table in facts.keys().filter(|t| t.as_str() != "aml_transactions") {
            assert!(
                analytics::read(&db, &database, network, table, Mode::Audit, 10)
                    .await?
                    .is_empty()
            );
        }
        core::publish_batch(&db, &database, &previous, &block, &facts, "{}").await?;
        for (table, rows) in &facts {
            if table != "aml_transactions" {
                assert_eq!(
                    analytics::read(&db, &database, network, table, Mode::Audit, 10).await?,
                    *rows
                );
            }
        }
        let mut foreign = fixture("aml_bitcoin_scripts", network)?;
        foreign["network_id"] = json!(if network == Network::Bitcoin {
            Network::Ethereum.id()
        } else {
            network.id()
        });
        assert!(
            db.row(&database, "aml_bitcoin_scripts", &foreign)
                .await
                .is_err(),
            "network-inapplicable script data accepted"
        );
    }
    Ok(())
}
struct Fail<'a> {
    db: &'a Http,
    prefix: &'a str,
    left: AtomicUsize,
}
impl Connection for Fail<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        if sql.starts_with(self.prefix) {
            ensure!(
                self.left.fetch_sub(1, Ordering::SeqCst) > 1,
                "simulated interruption"
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
async fn partial_new_tables_and_columns_resume_while_applied_drift_is_rejected() -> Result<()> {
    let db = Http::new()?;
    let network = Network::Bitcoin;
    for (name, prefix, count) in [
        ("columns", "ALTER TABLE", 5),
        ("tables", "CREATE TABLE", 4),
        ("views", "CREATE OR REPLACE VIEW", 25),
    ] {
        let database = format!("db05_partial_{name}");
        version_three(&db, &database, network).await?;
        assert!(
            initialize(
                &Fail {
                    db: &db,
                    prefix,
                    left: AtomicUsize::new(count)
                },
                &database,
                network
            )
            .await
            .is_err()
        );
        initialize(&db, &database, network).await?;
        validate(&db, &database, network).await?;
        db.execute(&format!(
            "ALTER TABLE {database}.aml_entities DROP CONSTRAINT aml_analytics_v4"
        ))
        .await?;
        assert!(initialize(&db, &database, network).await.is_err());
    }
    let database = "db05_unknown_partial";
    version_three(&db, database, network).await?;
    db.execute(&format!("CREATE TABLE {database}.aml_entities (network_id String,revision UInt64) ENGINE=ReplacingMergeTree(revision) ORDER BY network_id")).await?;
    assert!(
        initialize(&db, database, network).await.is_err(),
        "partial new table drift was blessed"
    );
    Ok(())
}
