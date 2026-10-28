const LEGACY_METADATA_COLUMNS: &str = "`network_id`,`asset_id`,`symbol`,`name`,`decimals`,`asset_standard`,`source_id`,`source_reference`,`review_status`,`reviewed_by`,`evidence_refs`,`revision`,`created_at_unix_ms`,`total_supply_raw`,`code_hash`,`is_verified`,`metadata_status`,`observed_block`,`verified_at_unix_ms`,`reviewed_at_unix_ms`,`updated_at_unix_ms`,`raw_evidence_json`";
use aml_warehouse::{
    Connection, Network,
    analytics::{self, Mode},
    publication::{self, Block, State},
    visibility::{self, Snapshot},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};
mod support;
use support::fixture;
#[derive(Clone)]
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
    async fn raw(&self, database: &str, table: &str, row: &Value) -> Result<()> {
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
            .map(|s| {
                let v: Value = serde_json::from_str(s)?;
                v.as_object()
                    .and_then(|o| o.values().next())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("String column required")
            })
            .collect()
    }
}
#[test]
fn snapshots_and_confirmation_depth_are_exact_and_not_finality() -> Result<()> {
    assert_eq!(visibility::confirmed_tip(0, 1), Some(0));
    assert_eq!(visibility::confirmed_tip(4, 6), None);
    assert_eq!(visibility::confirmed_tip(5, 6), Some(0));
    assert_eq!(visibility::confirmed_tip(u64::MAX, 1), Some(u64::MAX));
    assert_eq!(visibility::confirmed_tip(5, 0), None);
    for network in Network::ALL {
        let state = State {
            network_id: network.id().into(),
            revision: 5,
            initialized: 1,
            start_block: 7,
            last_block_number: 8,
            last_block_hash: aml_warehouse::values::normalize_hash(network, &"a".repeat(64))?,
            observed_rpc_height: 10,
            published_at_unix_ms: 1,
        };
        let a = Snapshot::from_state(state.clone())?;
        let mut b = state;
        b.revision += 1;
        assert_ne!(a.id(), Snapshot::from_state(b.clone())?.id());
        assert_eq!(a.value()["finality"], "not_asserted");
        assert_eq!(a.value()["successor_blocks_observed"], 2);
        assert_eq!(a.value()["history_complete"], false);
        b.start_block = 9;
        assert!(Snapshot::from_state(b).is_err());
    }
    assert_eq!(
        aml_warehouse::analytics_contract()?.checksum()?,
        "955e0c5ff2e6bb7000793e66408312f4d0172d026f15f328a9431ee050b568ec"
    );
    let c = aml_warehouse::current_contract()?;
    assert_eq!((c.version, c.tables.len(), c.views.len()), (5, 39, 53));
    assert_eq!(c.tables.iter().map(|t| t.columns.len()).sum::<usize>(), 682);
    let sql = aml_warehouse::visibility_schema::sql_template()?;
    assert!(!sql.contains("DROP ") && !sql.contains("TRUNCATE "));
    Ok(())
}
fn hash(network: Network, n: u8) -> Result<String> {
    aml_warehouse::values::normalize_hash(network, &format!("{n:02x}").repeat(32))
}
fn block(
    network: Network,
    height: u64,
    n: u8,
    revision: u64,
    parent: &str,
) -> Result<(Block, BTreeMap<String, Vec<Value>>)> {
    let mut b = fixture("aml_ingested_blocks", network)?;
    b["block_number"] = json!(height);
    b["block_hash"] = json!(hash(network, n)?);
    b["parent_hash"] = json!(parent);
    b["commit_id"] = json!(format!("block-{n}-{revision}"));
    b["revision"] = json!(revision);
    b["canonical"] = json!(1);
    b["ingestion_status"] = json!("complete");
    b["transaction_count"] = json!(1);
    let mut tx = fixture("aml_transactions", network)?;
    for key in [
        "network_id",
        "block_number",
        "block_hash",
        "block_timestamp_unix_ms",
        "commit_id",
        "revision",
    ] {
        tx[key] = b[key].clone();
    }
    tx["tx_hash"] = json!(hash(network, n + 20)?);
    Ok((
        serde_json::from_value(b)?,
        BTreeMap::from([("aml_transactions".into(), vec![tx])]),
    ))
}
fn analysis(
    network: Network,
    snapshot: &Snapshot,
    family: &str,
    id: &str,
    revision: u64,
) -> Result<BTreeMap<String, Vec<Value>>> {
    let mut rows = BTreeMap::new();
    for table in visibility::family_tables(family)? {
        let mut r = fixture(table, network)?;
        let key = if family == "risk" {
            "assessment_id"
        } else {
            "run_id"
        };
        r[key] = json!(id);
        r["revision"] = json!(revision);
        if r.get("source_revision").is_some() {
            r["source_revision"] = json!(snapshot.state().revision);
            r["as_of_block"] = json!(snapshot.state().last_block_number);
        }
        if table.ends_with("_runs") {
            r["status"] = json!("complete");
            r["truncated"] = json!(0);
            r["completed_at_unix_ms"] = json!(1);
        }
        if *table == "aml_exposure_runs" {
            r["path_count"] = json!(1);
        }
        if *table == "aml_wallet_risk_assessments" {
            r["signal_count"] = json!(1);
            r["exposure_run_id"] = json!("exposure-a");
        }
        rows.insert((*table).into(), vec![r]);
    }
    Ok(rows)
}
async fn install_v4(db: &Http, database: &str, network: Network) -> Result<()> {
    ensure!(
        db.query(&format!(
            "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
        ))
        .await?
            == ["0"],
        "fixture must be empty"
    );
    db.execute(&format!("CREATE DATABASE {database}")).await?;
    let v4 = aml_warehouse::analytics_contract()?;
    for sql in v4.statements(database)? {
        db.execute(&sql).await?;
    }
    for c in [
        aml_warehouse::baseline_contract()?,
        aml_warehouse::column_contract()?,
        aml_warehouse::core_contract()?,
        v4.clone(),
    ] {
        let mut row = fixture("aml_schema_migrations", network)?;
        row["migration_id"] = json!(c.migration_id);
        row["schema_version"] = json!(c.version);
        row["checksum"] = json!(c.checksum()?);
        row["revision"] = json!(c.version);
        db.raw(database, "aml_schema_migrations", &row).await?;
    }
    let mut row = fixture("aml_warehouse_identity", network)?;
    row["schema_version"] = json!(4);
    row["checksum"] = json!(v4.checksum()?);
    row["revision"] = json!(4);
    db.raw(database, "aml_warehouse_identity", &row).await?;
    let mut legacy = fixture("aml_asset_metadata", network)?;
    let old_columns = &v4
        .tables
        .iter()
        .find(|t| t.name == "aml_asset_metadata")
        .unwrap()
        .columns;
    legacy
        .as_object_mut()
        .unwrap()
        .retain(|key, _| old_columns.iter().any(|c| &c.name == key));
    db.raw(database, "aml_asset_metadata", &legacy).await?;
    Ok(())
}
struct FailSeal<'a>(&'a Http);
impl Connection for FailSeal<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        ensure!(
            !sql.starts_with("INSERT INTO") || !sql.contains(".aml_analysis_publications "),
            "injected crash before analysis seal"
        );
        self.0.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.0.query(sql).await
    }
}
struct Moving<'a> {
    db: &'a Http,
    reads: AtomicUsize,
}
impl Connection for Moving<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.db.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        let mut rows = self.db.query(sql).await?;
        if sql.contains(".aml_sync_state_current WHERE")
            && self.reads.fetch_add(1, Ordering::SeqCst) == 1
        {
            let mut row: Value = serde_json::from_str(&rows[0])?;
            row["revision"] = json!("999999");
            rows[0] = row.to_string();
        }
        Ok(rows)
    }
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8 fixtures"]
async fn upgrade_visibility_reorg_and_derived_invalidation_on_four_networks() -> Result<()> {
    let db = Http::new()?;
    for network in Network::ALL {
        let prefix =
            std::env::var("DB07_FIXTURE_PREFIX").unwrap_or_else(|_| "db07_visibility".into());
        let database = format!("{prefix}_{}", network.name());
        aml_warehouse::contract::identifier(&database)?;
        install_v4(&db, &database, network).await?;
        let raw_before = db
            .request(&format!(
                "SELECT {LEGACY_METADATA_COLUMNS} FROM {database}.aml_asset_metadata FINAL FORMAT JSONEachRow"
            ))
            .await?;
        let summary = aml_warehouse::initialize(&db, &database, network).await?;
        assert_eq!(
            (
                summary.schema_version,
                summary.table_count,
                summary.view_count
            ),
            (5, 39, 53)
        );
        assert_eq!(
            db.request(&format!(
                "SELECT {LEGACY_METADATA_COLUMNS} FROM {database}.aml_asset_metadata FINAL FORMAT JSONEachRow"
            ))
            .await?,
            raw_before
        );
        assert!(
            !aml_warehouse::initialize(&db, &database, network)
                .await?
                .newly_applied
        );
        let initial = publication::state(&db, &database, network).await?;
        let (b7, rows) = block(network, 7, 7, 3, &hash(network, 6)?)?;
        assert!(
            publication::complete(&db, &database, &b7).await.is_err(),
            "a missing transaction family cannot be marked complete"
        );
        let s7 =
            aml_warehouse::core::publish_batch(&db, &database, &initial, &b7, &rows, "{}").await?;
        let (b8, mut rows) = block(network, 8, 8, 6, &b7.block_hash)?;
        let mut discovery = fixture("aml_metadata_discoveries", network)?;
        for key in [
            "network_id",
            "block_number",
            "block_hash",
            "block_timestamp_unix_ms",
            "commit_id",
            "revision",
        ] {
            discovery[key] = rows["aml_transactions"][0][key].clone();
        }
        discovery["discovery_id"] = json!("discovery-8");
        discovery["tx_hash"] = rows["aml_transactions"][0]["tx_hash"].clone();
        rows.insert("aml_metadata_discoveries".into(), vec![discovery]);
        let s8 = aml_warehouse::core::publish_batch(&db, &database, &s7, &b8, &rows, "{}").await?;
        let source = visibility::capture(&db, &database, network).await?;
        let mut metadata = fixture("aml_asset_metadata", network)?;
        metadata["revision"] = json!(50);
        metadata["observed_block"] = json!(8);
        metadata["source_revision"] = json!(s8.revision);
        metadata["observed_block_hash"] = json!(b8.block_hash);
        publication::insert(
            &db,
            &database,
            "aml_asset_metadata",
            std::slice::from_ref(&metadata),
        )
        .await?;
        assert_eq!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_asset_metadata",
                Mode::Current,
                50
            )
            .await?
            .len(),
            1
        );
        metadata["revision"] = json!(51);
        metadata["observed_block_hash"] = json!(hash(network, 99)?);
        publication::insert(
            &db,
            &database,
            "aml_asset_metadata",
            std::slice::from_ref(&metadata),
        )
        .await?;
        assert!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_asset_metadata",
                Mode::Current,
                50
            )
            .await?
            .is_empty(),
            "a newer wrong observed hash must not resurrect older metadata"
        );
        metadata["revision"] = json!(52);
        metadata["observed_block_hash"] = json!(b8.block_hash);
        publication::insert(&db, &database, "aml_asset_metadata", &[metadata]).await?;
        let mut job = fixture("aml_metadata_jobs", network)?;
        job["discovery_id"] = json!("discovery-8");
        job["discovered_block"] = json!(8);
        job["source_revision"] = json!(s8.revision);
        publication::insert(&db, &database, "aml_metadata_jobs", &[job]).await?;
        assert_eq!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_metadata_jobs",
                Mode::Current,
                50
            )
            .await?
            .len(),
            1
        );
        let unsealed = analysis(network, &source, "cluster", "unsealed", 10)?;
        for (table, rows) in &unsealed {
            publication::insert(&db, &database, table, rows).await?;
            assert!(
                analytics::read(&db, &database, network, table, Mode::Current, 50)
                    .await?
                    .is_empty()
            );
        }
        for (family, id) in [
            ("cluster", "cluster-a"),
            ("exposure", "exposure-a"),
            ("risk", "risk-a"),
        ] {
            visibility::publish_analysis(
                &db,
                &database,
                &source,
                family,
                id,
                20,
                &analysis(network, &source, family, id, 20)?,
            )
            .await?;
            for table in visibility::family_tables(family)? {
                assert_eq!(
                    analytics::read(&db, &database, network, table, Mode::Current, 50)
                        .await?
                        .len(),
                    1,
                    "{table}"
                );
            }
        }
        // Partial analysis output is never current, even while its source is current.
        assert!(
            visibility::publish_analysis(
                &FailSeal(&db),
                &database,
                &source,
                "cluster",
                "crashed",
                21,
                &analysis(network, &source, "cluster", "crashed", 21)?
            )
            .await
            .is_err()
        );
        assert_eq!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_cluster_runs",
                Mode::Current,
                50
            )
            .await?
            .len(),
            1
        );
        assert!(
            visibility::publish_analysis(
                &db,
                &database,
                &source,
                "cluster",
                "crashed",
                21,
                &analysis(network, &source, "cluster", "crashed", 21)?
            )
            .await
            .is_err()
        );
        let moving = Moving {
            db: &db,
            reads: AtomicUsize::new(0),
        };
        assert!(
            analytics::read(
                &moving,
                &database,
                network,
                "aml_cluster_runs",
                Mode::Current,
                50
            )
            .await
            .is_err()
        );
        // A complete marker without a publication fence changes neither aggregates nor analyses.
        let (b9, rows) = block(network, 9, 9, 9, &b8.block_hash)?;
        publication::reserve(&db, &database, &b9).await?;
        for (t, r) in &rows {
            publication::facts(&db, &database, &b9, t, r).await?;
        }
        aml_warehouse::core::complete(
            &db,
            &database,
            &b9,
            &BTreeMap::from([("aml_transactions".into(), 1)]),
            "{}",
        )
        .await?;
        assert_eq!(
            db.query(&format!(
                "SELECT toString(count()) FROM {database}.aml_transactions_canonical"
            ))
            .await?,
            ["2"]
        );
        assert_eq!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_exposure_paths",
                Mode::Current,
                50
            )
            .await?
            .len(),
            1
        );
        let rollback = State {
            revision: 12,
            last_block_number: 7,
            last_block_hash: b7.block_hash.clone(),
            ..s8.clone()
        };
        publication::publish(&db, &database, network, &s8, &rollback).await?;
        for table in ["aml_asset_metadata", "aml_metadata_jobs"] {
            assert!(
                analytics::read(&db, &database, network, table, Mode::Current, 50)
                    .await?
                    .is_empty(),
                "stale {table}"
            );
        }
        assert!(
            visibility::unchanged(&db, &database, &source)
                .await
                .is_err()
        );
        assert_eq!(db.query(&format!("SELECT toString(count()) FROM {database}.aml_ingested_blocks FINAL WHERE ingestion_status='orphaned'")).await?,["1"]);
        assert_eq!(
            db.query(&format!(
                "SELECT toString(count()) FROM {database}.aml_transactions_canonical"
            ))
            .await?,
            ["1"]
        );
        for family in ["cluster", "exposure", "risk"] {
            for table in visibility::family_tables(family)? {
                assert!(
                    analytics::read(&db, &database, network, table, Mode::Current, 50)
                        .await?
                        .is_empty(),
                    "stale {table}"
                );
                assert!(
                    !analytics::read(&db, &database, network, table, Mode::Audit, 50)
                        .await?
                        .is_empty()
                );
            }
        }
        let skip = State {
            revision: 15,
            last_block_number: 9,
            last_block_hash: b9.block_hash.clone(),
            ..rollback.clone()
        };
        assert!(
            publication::publish(&db, &database, network, &rollback, &skip)
                .await
                .is_err()
        );
        let (replacement, rows) = block(network, 8, 18, 15, &b7.block_hash)?;
        let mut wrong = replacement.clone();
        wrong.parent_hash = hash(network, 88)?;
        assert!(
            aml_warehouse::core::publish_batch(&db, &database, &rollback, &wrong, &rows, "{}")
                .await
                .is_err()
        );
        let branch = aml_warehouse::core::publish_batch(
            &db,
            &database,
            &rollback,
            &replacement,
            &rows,
            "{}",
        )
        .await?;
        assert_eq!(db.query(&format!("SELECT block_hash FROM {database}.aml_ingested_blocks_canonical WHERE block_number=8")).await?,std::slice::from_ref(&replacement.block_hash));
        assert!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_exposure_paths",
                Mode::Current,
                50
            )
            .await?
            .is_empty()
        );
        // A previous complete block cannot be published using a new fence revision.
        let stale9 = State {
            revision: 18,
            last_block_number: 9,
            last_block_hash: b9.block_hash.clone(),
            ..branch.clone()
        };
        assert!(
            publication::publish(&db, &database, network, &branch, &stale9)
                .await
                .is_err()
        );
        let fresh = visibility::capture(&db, &database, network).await?;
        assert_ne!(source.id(), fresh.id());
        visibility::publish_analysis(
            &db,
            &database,
            &fresh,
            "cluster",
            "cluster-new",
            30,
            &analysis(network, &fresh, "cluster", "cluster-new", 30)?,
        )
        .await?;
        visibility::invalidate_analysis(&db, &database, network, "cluster", "cluster-new", 31)
            .await?;
        assert!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_cluster_memberships",
                Mode::Current,
                50
            )
            .await?
            .is_empty()
        );
        visibility::publish_analysis(
            &db,
            &database,
            &fresh,
            "exposure",
            "exposure-new",
            30,
            &analysis(network, &fresh, "exposure", "exposure-new", 30)?,
        )
        .await?;
        let mut latest =
            analysis(network, &fresh, "exposure", "exposure-new", 31)?["aml_exposure_runs"][0]
                .clone();
        latest["status"] = json!("stale");
        publication::insert(&db, &database, "aml_exposure_runs", &[latest]).await?;
        assert!(
            analytics::read(
                &db,
                &database,
                network,
                "aml_exposure_paths",
                Mode::Current,
                50
            )
            .await?
            .is_empty(),
            "a later stale parent must suppress its paths"
        );
        // Reset retains all historical rows while the visible observed window becomes empty.
        let reset = State {
            revision: 18,
            initialized: 0,
            last_block_number: 0,
            last_block_hash: String::new(),
            ..branch.clone()
        };
        publication::publish(&db, &database, network, &branch, &reset).await?;
        assert_eq!(
            db.query(&format!(
                "SELECT toString(count()) FROM {database}.aml_transactions_canonical"
            ))
            .await?,
            ["0"]
        );
        assert_eq!(
            db.query(&format!(
                "SELECT toString(count()) FROM {database}.aml_transactions FINAL"
            ))
            .await?,
            ["4"]
        );
    }
    Ok(())
}

struct Interrupted<'a>(&'a Http);
impl Connection for Interrupted<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        ensure!(
            !sql.starts_with("CREATE OR REPLACE VIEW")
                || !sql.contains(".aml_analysis_publications_visible "),
            "injected migration interruption"
        );
        self.0.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.0.query(sql).await
    }
}
#[tokio::test]
#[ignore = "requires a fresh disposable ClickHouse 23.8 fixture"]
async fn interrupted_v5_resumes_and_applied_view_drift_is_rejected() -> Result<()> {
    let db = Http::new()?;
    let network = Network::Bsc;
    let prefix = std::env::var("DB07_FIXTURE_PREFIX").unwrap_or_else(|_| "db07_visibility".into());
    let database = format!("{prefix}_resume");
    aml_warehouse::contract::identifier(&database)?;
    install_v4(&db, &database, network).await?;
    let before = db
        .request(&format!(
            "SELECT {LEGACY_METADATA_COLUMNS} FROM {database}.aml_asset_metadata FINAL FORMAT JSONEachRow"
        ))
        .await?;
    assert!(
        aml_warehouse::initialize(&Interrupted(&db), &database, network)
            .await
            .is_err()
    );
    let history = db
        .query(&format!(
            "SELECT toString(max(schema_version)) FROM {database}.aml_schema_migrations FINAL"
        ))
        .await?;
    assert_eq!(history, ["4"]);
    assert_eq!(
        aml_warehouse::initialize(&db, &database, network)
            .await?
            .schema_version,
        5
    );
    assert_eq!(
        db.request(&format!(
            "SELECT {LEGACY_METADATA_COLUMNS} FROM {database}.aml_asset_metadata FINAL FORMAT JSONEachRow"
        ))
        .await?,
        before
    );
    db.execute(&format!(
        "CREATE OR REPLACE VIEW {database}.aml_analysis_publications_visible AS SELECT 1 AS broken"
    ))
    .await?;
    assert!(
        aml_warehouse::validate(&db, &database, network)
            .await
            .is_err()
    );
    assert!(
        aml_warehouse::initialize(&db, &database, network)
            .await
            .is_err()
    );
    Ok(())
}
