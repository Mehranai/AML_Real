//! DB-04 acceptance on empty fixture databases, using exact values and the same
//! core API on each network. No deployment credentials or legacy data are used.
use aml_warehouse::{
    Connection, Network, RawAmount,
    core::{self, Query, View},
    initialize,
    publication::{self, Block},
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
    url: String,
    client: reqwest::Client,
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
        let body = response.text().await?;
        ensure!(status.is_success(), "ClickHouse: {body}");
        Ok(body)
    }
}
fn block(network: Network) -> Result<Block> {
    let mut row = fixture("aml_ingested_blocks", network)?;
    row["canonical"] = json!(1);
    row["ingestion_status"] = json!("complete");
    row["revision"] = json!(3);
    row["transaction_count"] = json!(1);
    Ok(serde_json::from_value(row)?)
}
fn facts(block: &Block) -> Result<BTreeMap<String, Vec<Value>>> {
    let net = Network::parse(&block.network_id)?;
    let header = serde_json::to_value(block)?;
    [
        "aml_transactions",
        "aml_address_relationships",
        "aml_transaction_features",
        "aml_semantic_aml_events",
    ]
    .into_iter()
    .map(|table| {
        let mut row = fixture(table, net)?;
        for key in [
            "network_id",
            "block_number",
            "block_hash",
            "block_timestamp_unix_ms",
            "commit_id",
            "revision",
        ] {
            row[key] = header[key].clone();
        }
        row["raw_evidence_json"] =
            json!(r#"{"source":"original ? ' quote","large":"9007199254740993"}"#);
        Ok((table.into(), vec![row]))
    })
    .collect()
}
struct NoIo(AtomicUsize);
impl Connection for NoIo {
    async fn execute(&self, _: &str) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("unexpected write")
    }
    async fn query(&self, _: &str) -> Result<Vec<String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("unexpected read")
    }
}
#[tokio::test]
async fn malformed_batches_and_queries_fail_before_io() -> Result<()> {
    let db = NoIo(AtomicUsize::new(0));
    let block = block(Network::Ethereum)?;
    let state = publication::State {
        network_id: block.network_id.clone(),
        ..Default::default()
    };
    for change in [
        "foreign_network",
        "bad_header",
        "unknown_family",
        "missing_transactions",
        "bad_evidence",
    ] {
        let mut rows = facts(&block)?;
        match change {
            "foreign_network" => {
                rows.get_mut("aml_transactions").unwrap()[0]["network_id"] =
                    json!(Network::Bsc.id())
            }
            "bad_header" => rows.get_mut("aml_transactions").unwrap()[0]["revision"] = json!(4),
            "unknown_family" => {
                rows.insert("aml_sync_state".into(), vec![]);
            }
            "missing_transactions" => {
                rows.remove("aml_transactions");
            }
            _ => {}
        }
        assert!(
            core::publish_batch(
                &db,
                "fixture",
                &state,
                &block,
                &rows,
                if change == "bad_evidence" {
                    "invalid"
                } else {
                    "{}"
                }
            )
            .await
            .is_err(),
            "{change}"
        );
    }
    assert_eq!(db.0.load(Ordering::SeqCst), 0);
    let invalid_state = publication::State {
        revision: block.revision,
        ..state.clone()
    };
    assert!(
        core::publish_batch(
            &db,
            "fixture",
            &invalid_state,
            &block,
            &facts(&block)?,
            "{}"
        )
        .await
        .is_err()
    );
    assert_eq!(db.0.load(Ordering::SeqCst), 0);
    for query in [
        Query {
            limit: 0,
            ..Default::default()
        },
        Query {
            limit: 501,
            ..Default::default()
        },
        Query {
            from: Some(5),
            to: Some(4),
            ..Default::default()
        },
    ] {
        assert!(core::select("fixture", Network::Bitcoin, View::Blocks, &query).is_err());
    }
    assert!(
        core::select(
            "fixture",
            Network::Bitcoin,
            View::State,
            &Query {
                from: Some(1),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        core::select(
            "fixture; DROP DATABASE x",
            Network::Bitcoin,
            View::Blocks,
            &Query::default()
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn core_extensions_preserve_nullable_unknowns_nft_identity_and_trace_order() -> Result<()> {
    let contract = aml_warehouse::core_contract()?;
    assert_eq!(contract.tables.len(), 29);
    assert_eq!(contract.views.len(), 21);
    assert_eq!(
        contract
            .tables
            .iter()
            .map(|t| t.columns.len())
            .sum::<usize>(),
        404
    );
    let failure = fixture("aml_ingestion_failures", Network::Bitcoin)?;
    for key in [
        "retryable",
        "first_failed_at_unix_ms",
        "last_failed_at_unix_ms",
        "resolved_at_unix_ms",
        "tx_hash",
    ] {
        assert!(failure[key].is_null());
    }
    let mut row = fixture("aml_address_relationships", Network::Ethereum)?;
    row["asset_id"] = json!(format!("erc1155:0x{}/{}", "a".repeat(40), RawAmount::MAX));
    row["token_id"] = json!(RawAmount::MAX);
    row["trace_address"] = json!([3, 0, 4294967295u32]);
    row["event_sub_index"] = json!(17);
    aml_warehouse::semantics::validate_row(&contract, "aml_address_relationships", &row)?;
    row["token_id"] = json!("1");
    assert!(
        aml_warehouse::semantics::validate_row(&contract, "aml_address_relationships", &row)
            .is_err()
    );
    row["token_id"] = Value::Null;
    row["trace_address"] = json!([4294967296u64]);
    assert!(
        aml_warehouse::semantics::validate_row(&contract, "aml_address_relationships", &row)
            .is_err()
    );
    let mut bench = fixture("aml_ingestion_benchmarks", Network::Bitcoin)?;
    bench["rows_per_second"] = json!(-1);
    assert!(
        aml_warehouse::semantics::validate_row(&contract, "aml_ingestion_benchmarks", &bench)
            .is_err()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn identical_core_queries_on_four_databases_preserve_evidence_and_fence_partial_facts()
-> Result<()> {
    let db = Http {
        url: std::env::var("WAREHOUSE_TEST_URL")?,
        client: reqwest::Client::new(),
    };
    let query = Query::default();
    let evidence = r#"{"original":"? ' quote","legacy_only":{"value":"9007199254740993"}}"#;
    for network in Network::ALL {
        let database = format!("db04_core_{}", network.name());
        ensure!(
            db.query(&format!(
                "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
            ))
            .await?
                == ["0"],
            "fixture must be empty"
        );
        initialize(&db, &database, network).await?;
        let previous = publication::state(&db, &database, network).await?;
        let block = block(network)?;
        let mut rows = facts(&block)?;
        if matches!(network, Network::Ethereum | Network::Bsc) {
            let relation = &mut rows.get_mut("aml_address_relationships").unwrap()[0];
            relation["asset_id"] =
                json!(format!("erc1155:0x{}/{}", "a".repeat(40), RawAmount::MAX));
            relation["token_id"] = json!(RawAmount::MAX);
            relation["trace_address"] = json!([4, 0, 7]);
            relation["event_sub_index"] = json!(3);
            relation["amount_raw"] = json!(RawAmount::MAX);
        }
        publication::reserve(&db, &database, &block).await?;
        for (table, values) in &rows {
            publication::facts(&db, &database, &block, table, values).await?;
        }
        for view in [
            View::Blocks,
            View::Transactions,
            View::Relationships,
            View::Features,
            View::Events,
        ] {
            assert!(
                core::read(&db, &database, network, view, &query)
                    .await?
                    .is_empty()
            );
        }
        let wrong_counts = BTreeMap::from([("aml_transactions".to_owned(), 2)]);
        assert!(
            core::complete(&db, &database, &block, &wrong_counts, evidence)
                .await
                .is_err()
        );
        core::publish_batch(&db, &database, &previous, &block, &rows, evidence).await?;
        for table in ["aml_ingestion_failures", "aml_ingestion_benchmarks"] {
            let mut row = fixture(table, network)?;
            if table == "aml_ingestion_benchmarks" {
                row["status"] = json!("running");
                row["revision"] = json!(4);
                row["metrics_json"] = json!(evidence);
            }
            publication::insert(&db, &database, table, &[row.clone()]).await?;
            if table == "aml_ingestion_benchmarks" {
                row["status"] = json!("complete");
                row["revision"] = json!(5);
                row["rows_per_second"] = json!(12.5);
                publication::insert(&db, &database, table, &[row]).await?;
            }
        }
        for view in View::ALL {
            let read = core::read(&db, &database, network, view, &query).await?;
            assert_eq!(read.len(), 1, "{} {}", network.name(), view.name());
            assert!(
                core::read(
                    &db,
                    &database,
                    if network == Network::Bitcoin {
                        Network::Ethereum
                    } else {
                        Network::Bitcoin
                    },
                    view,
                    &query
                )
                .await?
                .is_empty()
            );
        }
        let blocks = core::read(&db, &database, network, View::Blocks, &query).await?;
        assert_eq!(blocks[0]["raw_evidence_json"], evidence);
        let counts: Value =
            serde_json::from_str(blocks[0]["expected_counts_json"].as_str().unwrap())?;
        assert_eq!(counts["aml_transactions"], 1);
        assert!(counts.get("aml_evm_logs").is_none());
        let relations = core::read(&db, &database, network, View::Relationships, &query).await?;
        assert_eq!(relations[0], rows["aml_address_relationships"][0]);
        let benches = core::read(&db, &database, network, View::Benchmarks, &query).await?;
        assert_eq!(benches[0]["status"], "complete");
        assert_eq!(benches[0]["rows_per_second"], 12.5);
        // Facts with a mismatched commit never enter a published core view.
        let mut wrong = rows["aml_transactions"][0].clone();
        wrong["commit_id"] = json!("unpublished-other-commit");
        wrong["revision"] = json!(4);
        publication::insert(&db, &database, "aml_transactions", &[wrong]).await?;
        assert_eq!(
            core::read(&db, &database, network, View::Transactions, &query)
                .await?
                .len(),
            1
        );
    }
    Ok(())
}
