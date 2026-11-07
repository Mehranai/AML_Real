//! Shared live writer/read boundary tests on isolated four-network databases.
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
    let mut families = vec![
        "aml_transactions",
        "aml_address_relationships",
        "aml_transaction_features",
        "aml_semantic_aml_events",
        "aml_raw_transactions",
    ];
    if matches!(net, Network::Ethereum | Network::Bsc) {
        families.extend(["aml_evm_logs", "aml_evm_traces"]);
    } else if net == Network::Tron {
        families.extend([
            "aml_tron_contracts",
            "aml_tron_logs",
            "aml_tron_internals",
            "aml_tron_resources",
        ]);
    }
    families
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
            if net == Network::Bitcoin && table == "aml_address_relationships" {
                let tx = row["tx_hash"].as_str().unwrap();
                let index = row["event_index"].as_u64().unwrap();
                row["relationship_id"] = json!(format!("{}:{tx}:out:{index}", block.block_hash));
                row["from_subject_id"] =
                    json!(aml_warehouse::relationships::transaction_subject_id(
                        net,
                        &block.block_hash,
                        row["tx_hash"].as_str().unwrap()
                    )?);
            }
            if row.get("raw_evidence_json").is_some() {
                row["raw_evidence_json"] =
                    json!(r#"{"source":"original ? ' quote","large":"9007199254740993"}"#);
            }
            if table == "aml_raw_transactions" {
                row["payload_json"] =
                    json!(r#"{"unknown":"original ? ' quote","large":"9007199254740993"}"#);
            }
            if table == "aml_evm_traces" {
                row["trace_address"] = json!([2, 0, 4294967295u32]);
                row["value_raw"] = json!(RawAmount::MAX);
            }
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
#[tokio::test]
async fn live_batch_integrity_is_checked_before_any_io() -> Result<()> {
    let db = NoIo(AtomicUsize::new(0));
    for network in Network::ALL {
        let original = block(network)?;
        let previous = publication::State {
            network_id: network.id().into(),
            ..Default::default()
        };
        core::validate_batch(&previous, &original, &facts(&original)?, "{}")?;
        for defect in [
            "duplicate_transaction",
            "gapped_position",
            "orphan_fact",
            "wrong_position",
            "duplicate_fact",
            "wrong_profile",
            "wrong_coverage",
            "numeric_amount",
            "invalid_subject",
        ] {
            let mut b = original.clone();
            let mut rows = facts(&b)?;
            match defect {
                "duplicate_transaction" => {
                    let tx = rows["aml_transactions"][0].clone();
                    rows.get_mut("aml_transactions").unwrap().push(tx);
                    b.transaction_count = 2;
                }
                "gapped_position" => {
                    rows.get_mut("aml_transactions").unwrap()[0]["transaction_index"] = json!(1)
                }
                "orphan_fact" => {
                    rows.get_mut("aml_transaction_features").unwrap()[0]["tx_hash"] = json!(
                        aml_warehouse::values::normalize_hash(network, &"b".repeat(64))?
                    )
                }
                "wrong_position" => {
                    rows.get_mut("aml_address_relationships").unwrap()[0]["transaction_index"] =
                        json!(1)
                }
                "duplicate_fact" => {
                    let mut duplicate = rows["aml_address_relationships"][0].clone();
                    duplicate["amount_raw"] = json!("7");
                    rows.get_mut("aml_address_relationships")
                        .unwrap()
                        .push(duplicate);
                }
                "wrong_profile" => {
                    let family = if network == Network::Bitcoin {
                        "aml_tron_contracts"
                    } else {
                        "aml_bitcoin_outputs"
                    };
                    rows.insert(family.into(), vec![fixture(family, network)?]);
                }
                "wrong_coverage" => {
                    if network == Network::Bitcoin {
                        b.receipt_coverage = Some(1);
                    } else {
                        b.prevout_coverage = Some(1);
                    }
                }
                "numeric_amount" => {
                    rows.get_mut("aml_address_relationships").unwrap()[0]["amount_raw"] =
                        json!(9007199254740993u64)
                }
                "invalid_subject" => {
                    rows.get_mut("aml_address_relationships").unwrap()[0]["to_address"] =
                        json!("invented")
                }
                _ => unreachable!(),
            }
            let error = core::publish_batch(&db, "fixture", &previous, &b, &rows, "{}")
                .await
                .unwrap_err();
            assert!(
                !error.to_string().starts_with("unexpected"),
                "{network:?} {defect}: {error:#}"
            );
            assert_eq!(db.0.load(Ordering::SeqCst), 0, "{network:?} {defect}");
        }
        let mut empty_profile = facts(&original)?;
        empty_profile.insert("aml_tron_contracts".into(), vec![]);
        core::validate_batch(&previous, &original, &empty_profile, "{}")?;
        assert!(
            core::publish_observed_batch(
                &db,
                "fixture",
                &previous,
                &original,
                &facts(&original)?,
                "{}",
                0
            )
            .await
            .is_err()
        );
        assert_eq!(db.0.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

struct CorruptWrites<'a>(&'a Http);
impl Connection for CorruptWrites<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.0.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.0.query(sql).await
    }
    async fn insert_json(&self, table: &str, body: &str) -> Result<()> {
        if table.ends_with(".aml_address_relationships") {
            let mut changed = String::new();
            for line in body.lines() {
                let mut row: Value = serde_json::from_str(line)?;
                row["amount_raw"] = json!("7");
                changed.push_str(&serde_json::to_string(&row)?);
                changed.push('\n');
            }
            self.0.insert_json(table, &changed).await
        } else {
            self.0.insert_json(table, body).await
        }
    }
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
        assert_eq!(
            counts.get("aml_evm_logs").cloned(),
            if matches!(network, Network::Ethereum | Network::Bsc) {
                Some(json!(1))
            } else {
                None
            }
        );
        let relations = core::read(&db, &database, network, View::Relationships, &query).await?;
        assert_eq!(relations[0], rows["aml_address_relationships"][0]);
        let committed = publication::state(&db, &database, network).await?;
        let mut next = block.clone();
        next.block_number += 1;
        next.block_hash = aml_warehouse::values::normalize_hash(network, &"b".repeat(64))?;
        next.parent_hash = block.block_hash.clone();
        next.commit_id = "corrupted-write".into();
        next.revision =
            publication::next_revision(&db, &database, network, committed.revision).await?;
        let next_rows = facts(&next)?;
        let error = core::publish_observed_batch(
            &CorruptWrites(&db),
            &database,
            &committed,
            &next,
            &next_rows,
            "{}",
            100,
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("persisted core evidence differs"),
            "{error:#}"
        );
        assert_eq!(
            publication::state(&db, &database, network).await?,
            committed
        );
        assert_eq!(
            core::read(&db, &database, network, View::Transactions, &query)
                .await?
                .len(),
            1
        );
        next.revision =
            publication::next_revision(&db, &database, network, committed.revision).await?;
        next.commit_id = "verified-retry".into();
        let next_rows = facts(&next)?;
        let retried =
            core::publish_observed_batch(&db, &database, &committed, &next, &next_rows, "{}", 100)
                .await?;
        assert_eq!(retried.observed_rpc_height, 100);
        assert_eq!(
            core::read(&db, &database, network, View::Transactions, &query)
                .await?
                .len(),
            2
        );

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
            2
        );
    }
    Ok(())
}
