//! Run against a disposable ClickHouse 23.8 instance only.
//! WAREHOUSE_TEST_URL=http://... cargo test --test clickhouse_contract -- --ignored
use aml_warehouse::{Connection, Network, current_contract, initialize, validate};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
mod support;
use support::fixture;

async fn insert(db: &Http, database: &str, table: &str, row: &Value) -> Result<()> {
    db.execute(&format!(
        "INSERT INTO {database}.{table} FORMAT JSONEachRow\n{}",
        serde_json::to_string(row)?
    ))
    .await
}

struct Http {
    url: String,
    client: reqwest::Client,
}
impl Http {
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
        let text = response.text().await?;
        ensure!(status.is_success(), "ClickHouse: {text}");
        Ok(text)
    }
}
impl Connection for Http {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.request(sql).await?;
        Ok(())
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        let text = self.request(&format!("{sql} FORMAT JSONEachRow")).await?;
        text.lines()
            .map(|line| {
                let row: Value = serde_json::from_str(line)?;
                row.as_object()
                    .and_then(|v| v.values().next())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("expected a single String column")
            })
            .collect()
    }
}
struct FailAfter<'a> {
    db: &'a Http,
    remaining: AtomicUsize,
}
impl Connection for FailAfter<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        ensure!(
            self.remaining.fetch_sub(1, Ordering::SeqCst) > 1,
            "simulated schema job crash"
        );
        self.db.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.db.query(sql).await
    }
}
async fn scalar(db: &Http, sql: &str) -> Result<String> {
    db.query(sql).await?.pop().context("empty scalar result")
}

#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8 instance"]
async fn publication_helpers_fence_partial_writes_and_scope_every_network() -> Result<()> {
    use aml_warehouse::publication::{self, Block};
    use serde_json::json;
    let db = Http {
        url: std::env::var("WAREHOUSE_TEST_URL")?,
        client: reqwest::Client::new(),
    };
    for network in Network::ALL {
        let database = format!("warehouse_publication_{}", network.name());
        initialize(&db, &database, network).await?;
        let previous = publication::state(&db, &database, network).await?;
        ensure!(previous.revision == 0, "publication fixture must be empty");
        let revision = publication::next_revision(&db, &database, network, 0).await?;
        let mut block = Block {
            network_id: network.id().into(),
            block_number: 7,
            block_hash: aml_warehouse::values::normalize_hash(network, &"a".repeat(64))?,
            block_timestamp_unix_ms: 123000,
            commit_id: "attempt-1".into(),
            revision,
            parent_hash: aml_warehouse::values::normalize_hash(network, &"b".repeat(64))?,
            canonical: 1,
            ingestion_status: "complete".into(),
            transaction_count: 1,
            receipt_coverage: None,
            trace_coverage: None,
            prevout_coverage: None,
            source_id: "test".into(),
            created_at_unix_ms: 1,
        };
        let row = json!({"network_id":network.id(),"block_number":7,"block_hash":block.block_hash,"block_timestamp_unix_ms":123000,"commit_id":"attempt-1","revision":revision,"tx_hash":aml_warehouse::values::normalize_hash(network,&"c".repeat(64))?,"transaction_index":0,"from_address":null,"to_address":null,"amount_raw":if network==Network::Bitcoin {None}else{Some(aml_warehouse::RawAmount::MAX)},"fee_raw":"1","asset_id":network.native_asset(),"decimals":network.native_decimals(),"status":"confirmed","coinbase":0,"input_count":1,"output_count":1,"raw_evidence_json":"{\"quoted\":\"'\\\\\"}","created_at_unix_ms":1});
        publication::reserve(&db, &database, &block).await?;
        let mut missing = row.clone();
        missing.as_object_mut().unwrap().remove("fee_raw");
        assert!(
            publication::facts(&db, &database, &block, "aml_transactions", &[missing])
                .await
                .is_err()
        );
        let mut foreign = row.clone();
        foreign["network_id"] = json!("foreign:network");
        assert!(
            publication::facts(&db, &database, &block, "aml_transactions", &[foreign])
                .await
                .is_err()
        );
        publication::facts(
            &db,
            &database,
            &block,
            "aml_transactions",
            std::slice::from_ref(&row),
        )
        .await?;
        publication::complete(&db, &database, &block).await?;
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT toString(count()) FROM {database}.aml_transactions_canonical")
            )
            .await?,
            "0"
        );
        let next = publication::State {
            network_id: network.id().into(),
            revision,
            initialized: 1,
            start_block: 7,
            last_block_number: 7,
            last_block_hash: block.block_hash.clone(),
            observed_rpc_height: 7,
            published_at_unix_ms: 1,
        };
        publication::publish(&db, &database, network, &previous, &next).await?;
        assert_eq!(
            scalar(
                &db,
                &format!(
                    "SELECT toString({}) FROM {database}.aml_transactions_canonical",
                    if network == Network::Bitcoin {
                        "fee_raw"
                    } else {
                        "amount_raw"
                    }
                )
            )
            .await?,
            if network == Network::Bitcoin {
                "1"
            } else {
                aml_warehouse::RawAmount::MAX
            }
        );
        let mut stale = next.clone();
        stale.revision += 10;
        assert!(
            publication::publish(&db, &database, network, &previous, &stale)
                .await
                .is_err()
        );
        // An interrupted attempt reserves r-1 and writes facts at r. A restart
        // must allocate both its new reservation and facts strictly after r.
        block.block_number = 8;
        block.revision = 100;
        block.commit_id = "crashed".into();
        publication::reserve(&db, &database, &block).await?;
        let mut partial = row.clone();
        partial["block_number"] = json!(8);
        partial["revision"] = json!(100);
        partial["commit_id"] = json!("crashed");
        publication::facts(&db, &database, &block, "aml_transactions", &[partial]).await?;
        let resumed = publication::next_revision(&db, &database, network, revision).await?;
        assert!(resumed - 1 > 100);
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT toString(count()) FROM {database}.aml_transactions_canonical")
            )
            .await?,
            "1"
        );
        let reset = publication::State {
            revision: resumed,
            initialized: 0,
            last_block_number: 0,
            last_block_hash: String::new(),
            ..next.clone()
        };
        publication::publish(&db, &database, network, &next, &reset).await?;
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT toString(count()) FROM {database}.aml_transactions_canonical")
            )
            .await?,
            "0"
        );
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT toString(count()) FROM {database}.aml_transactions FINAL")
            )
            .await?,
            "2"
        );
        block.network_id = Network::ALL
            .into_iter()
            .find(|other| other.id() != network.id())
            .unwrap()
            .id()
            .into();
        assert!(publication::reserve(&db, &database, &block).await.is_err());
        assert!(publication::complete(&db, &database, &block).await.is_err());
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8 instance"]
async fn identical_contract_upgrade_guards_and_canonical_visibility() -> Result<()> {
    let db = Http {
        url: std::env::var("WAREHOUSE_TEST_URL")?,
        client: reqwest::Client::new(),
    };
    let contract = current_contract()?;
    let mut shapes = Vec::new();
    for network in Network::ALL {
        let name = format!("warehouse_test_{}", network.name());
        ensure!(
            scalar(
                &db,
                &format!("SELECT toString(count()) FROM system.tables WHERE database='{name}'")
            )
            .await?
                == "0",
            "test database must be empty"
        );
        db.execute(&format!("CREATE DATABASE {name}")).await?;
        db.execute(&format!("CREATE TABLE {name}.transactions (legacy_evidence String) ENGINE=MergeTree ORDER BY legacy_evidence")).await?;
        db.execute(&format!(
            "INSERT INTO {name}.transactions VALUES ('keep existing evidence')"
        ))
        .await?;
        let first = if network == Network::Bitcoin {
            aml_warehouse::migration::initialize_from_sql(
                &db,
                &name,
                network,
                &aml_warehouse::baseline_contract()?.sql_template()?,
            )
            .await?
        } else {
            initialize(&db, &name, network).await?
        };
        assert!(first.newly_applied);
        assert_eq!(first.table_count, contract.tables.len());
        assert!(!initialize(&db, &name, network).await?.newly_applied);
        validate(&db, &name, network).await?;
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT legacy_evidence FROM {name}.transactions")
            )
            .await?,
            "keep existing evidence"
        );
        let shape=db.query(&format!("SELECT toJSONString(map('table',table,'name',name,'type',type,'position',toString(position))) AS row FROM system.columns WHERE database='{name}' AND startsWith(table,'aml_') ORDER BY table,position")).await?;
        shapes.push(shape);
    }
    assert!(
        shapes.windows(2).all(|pair| pair[0] == pair[1]),
        "all four databases must have exactly the same column contract"
    );
    assert!(
        initialize(&db, "warehouse_test_tron", Network::Bitcoin)
            .await
            .is_err()
    );

    let partial = "warehouse_test_partial";
    assert!(
        initialize(
            &FailAfter {
                db: &db,
                remaining: AtomicUsize::new(8)
            },
            partial,
            Network::Bitcoin
        )
        .await
        .is_err()
    );
    assert!(
        initialize(&db, partial, Network::Bitcoin)
            .await?
            .newly_applied
    );
    validate(&db, partial, Network::Bitcoin).await?;

    let name = "warehouse_test_bitcoin";
    let mut transaction = fixture("aml_transactions", Network::Bitcoin)?;
    transaction["revision"] = serde_json::json!(10);
    transaction["commit_id"] = serde_json::json!("complete");
    insert(&db, name, "aml_transactions", &transaction).await?;
    let mut pending = transaction.clone();
    pending["commit_id"] = serde_json::json!("unpublished");
    pending["revision"] = serde_json::json!(11);
    pending["tx_hash"] = serde_json::json!("b".repeat(64));
    insert(&db, name, "aml_transactions", &pending).await?;
    let mut marker = fixture("aml_ingested_blocks", Network::Bitcoin)?;
    marker["revision"] = serde_json::json!(10);
    marker["commit_id"] = serde_json::json!("complete");
    marker["ingestion_status"] = serde_json::json!("complete");
    insert(&db, name, "aml_ingested_blocks", &marker).await?;
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT toString(count()) FROM {name}.aml_transactions_canonical")
        )
        .await?,
        "0"
    );
    let mut fence = fixture("aml_sync_state", Network::Bitcoin)?;
    fence["revision"] = serde_json::json!(10);
    insert(&db, name, "aml_sync_state", &fence).await?;
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT toString(count()) FROM {name}.aml_transactions_canonical")
        )
        .await?,
        "1"
    );
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT toString(fee_raw) FROM {name}.aml_transactions_canonical")
        )
        .await?,
        "1"
    );
    // Lowering the fence invalidates every fact without deleting evidence.
    fence["revision"] = serde_json::json!(12);
    fence["last_block_number"] = serde_json::json!(0);
    fence["last_block_hash"] = serde_json::json!("c".repeat(64));
    insert(&db, name, "aml_sync_state", &fence).await?;
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT toString(count()) FROM {name}.aml_transactions_canonical")
        )
        .await?,
        "0"
    );
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT toString(count()) FROM {name}.aml_transactions FINAL")
        )
        .await?,
        "2"
    );

    db.execute(&format!("CREATE OR REPLACE VIEW {name}.aml_transactions_canonical AS SELECT * FROM {name}.aml_transactions FINAL")).await?;
    assert!(
        validate(&db, name, Network::Bitcoin).await.is_err(),
        "view predicate drift must be detected even when its columns still match"
    );
    assert!(
        initialize(&db, name, Network::Bitcoin).await.is_err(),
        "initialization must not silently overwrite drift"
    );
    let restore = contract
        .statements(name)?
        .into_iter()
        .find(|s| {
            s.starts_with(&format!(
                "CREATE VIEW IF NOT EXISTS {name}.aml_transactions_canonical "
            ))
        })
        .unwrap();
    db.execute(&restore.replacen("CREATE VIEW IF NOT EXISTS", "CREATE OR REPLACE VIEW", 1))
        .await?;
    validate(&db, name, Network::Bitcoin).await?;

    db.execute(&format!(
        "ALTER TABLE {name}.aml_transactions ADD COLUMN rogue UInt8"
    ))
    .await?;
    assert!(validate(&db, name, Network::Bitcoin).await.is_err());
    db.execute(&format!(
        "ALTER TABLE {name}.aml_transactions DROP COLUMN rogue"
    ))
    .await?;
    validate(&db, name, Network::Bitcoin).await?;

    // Exactly the same query works in all four databases. Selecting a different
    // network never returns the local network's canonical evidence.
    for network in Network::ALL {
        let database = format!("warehouse_test_{}", network.name());
        for table in ["aml_transactions", "aml_ingested_blocks", "aml_sync_state"] {
            let mut row = fixture(table, network)?;
            row["revision"] = serde_json::json!(30);
            if table == "aml_sync_state" {
                row["start_block"] = serde_json::json!(0);
                row["last_block_number"] = serde_json::json!(0);
            } else {
                row["block_number"] = serde_json::json!(0);
                row["commit_id"] = serde_json::json!("scope-commit");
            }
            if table == "aml_ingested_blocks" {
                row["ingestion_status"] = serde_json::json!("complete");
            }
            insert(&db, &database, table, &row).await?;
        }
        for requested in Network::ALL {
            let count = scalar(&db,&format!("SELECT toString(count()) FROM {database}.aml_transactions_canonical WHERE network_id='{}'",requested.id())).await?;
            assert_eq!(count, if requested == network { "1" } else { "0" });
        }
    }

    let ledger = "warehouse_test_bsc";
    db.execute(&format!("INSERT INTO {ledger}.aml_schema_migrations (network_id,migration_id,schema_version,checksum,revision,created_at_unix_ms) VALUES ('{}','{}',4,'tampered',18446744073709551615,1)",Network::Bsc.id(),contract.migration_id)).await?;
    assert!(initialize(&db, ledger, Network::Bsc).await.is_err());
    Ok(())
}
