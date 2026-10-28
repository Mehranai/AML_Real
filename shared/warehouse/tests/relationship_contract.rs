use aml_warehouse::{
    Connection, Network, RawAmount, core, initialize,
    publication::{self, Block},
    relationships::{self, Edge, Graph, Kind, Query, Subject},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};
mod support;
use support::fixture;

fn account(network: Network, side: bool) -> String {
    match network {
        Network::Ethereum | Network::Bsc => {
            format!("0x{}", if side { "1" } else { "2" }.repeat(40))
        }
        Network::Tron => aml_warehouse::values::tron_wallet(&format!(
            "41{}",
            if side { "1" } else { "2" }.repeat(40)
        ))
        .unwrap(),
        Network::Bitcoin => if side {
            "1BoatSLRHtKNngkdXEeobR76b53LETtpyT"
        } else {
            "1BitcoinEaterAddressDontSendf59kuE"
        }
        .into(),
    }
}
fn row(network: Network, input: bool, index: u32) -> Result<Value> {
    let mut r = fixture("aml_address_relationships", network)?;
    r["block_number"] = json!(7);
    r["block_timestamp_unix_ms"] = json!(123000);
    r["revision"] = json!(3);
    r["commit_id"] = json!("observed ? ' commit");
    r["event_index"] = json!(index);
    r["event_sub_index"] = json!(0);
    r["transaction_index"] = json!(1);
    r["transfer_type"] = json!("native_transfer");
    r["attribution_method"] = json!("observed_transfer");
    r["confidence"] = json!(1.0);
    r["amount_raw"] = json!("1");
    r["prev_tx_hash"] = Value::Null;
    r["prev_output_index"] = Value::Null;
    r["output_index"] = Value::Null;
    r["from_subject_type"] = json!("address");
    r["to_subject_type"] = json!("address");
    r["from_subject_id"] = json!(account(network, true));
    r["to_subject_id"] = json!(account(network, false));
    r["from_address"] = r["from_subject_id"].clone();
    r["to_address"] = r["to_subject_id"].clone();
    if network == Network::Bitcoin {
        let script = format!("6a01{:02x}", index + u32::from(!input) * 16);
        let wallet = relationships::script_subject_id(&script)?;
        let tx = relationships::transaction_subject_id(
            network,
            r["block_hash"].as_str().unwrap(),
            r["tx_hash"].as_str().unwrap(),
        )?;
        r["relationship_id"] = json!(format!(
            "{}:{}:{}:{index}",
            r["block_hash"].as_str().unwrap(),
            r["tx_hash"].as_str().unwrap(),
            if input { "in" } else { "out" }
        ));
        r["from_address"] = Value::Null;
        r["to_address"] = Value::Null;
        r["from_subject_type"] = json!(if input { "script" } else { "transaction" });
        r["to_subject_type"] = json!(if input { "transaction" } else { "script" });
        r["from_subject_id"] = json!(if input { wallet.clone() } else { tx.clone() });
        r["to_subject_id"] = json!(if input { tx } else { wallet });
        r["transfer_type"] = json!(if input { "utxo_input" } else { "utxo_output" });
        r["attribution_method"] = json!(if input {
            "observed_prevout"
        } else {
            "observed_output"
        });
        r["amount_raw"] = json!(if input { "2" } else { "1" });
        if input {
            r["prev_tx_hash"] = json!("b".repeat(64));
            r["prev_output_index"] = json!(index);
        } else {
            r["output_index"] = json!(index);
        }
        r["evidence_refs"] = json!([format!(
            "outpoint:{}:{index}",
            if input {
                "b".repeat(64)
            } else {
                r["tx_hash"].as_str().unwrap().into()
            }
        )]);
    }
    Ok(r)
}
#[test]
fn multi_input_and_script_edges_are_observed_legs_without_allocated_pairs() -> Result<()> {
    let mut graph = Graph::new(Network::Bitcoin);
    for (input, n) in [(true, 2), (false, 3)] {
        for i in 0..n {
            graph.edge(Edge::from_row(row(Network::Bitcoin, input, i)?)?)?;
        }
    }
    let value = graph.value();
    let edges = value["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 5);
    assert_eq!(value["nodes"].as_array().unwrap().len(), 6);
    assert!(
        value["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["address"].is_null())
    );
    assert!(edges.iter().all(|e| e["fund_continuity_claimed"] == false));
    assert_eq!(
        edges
            .iter()
            .filter(|e| e["transfer_type"] == "utxo_input")
            .map(|e| e["amount"].as_str().unwrap().parse::<u64>().unwrap())
            .sum::<u64>(),
        4
    );
    assert!(
        !edges
            .iter()
            .any(|e| e["from_subject_type"] == "address" && e["to_subject_type"] == "address")
    );
    assert_eq!(
        edges
            .iter()
            .filter(|e| e["transfer_type"] == "utxo_input")
            .count(),
        2
    );
    Ok(())
}
#[test]
fn inconsistent_types_outpoints_and_allocated_bitcoin_transfers_are_rejected() -> Result<()> {
    let original = row(Network::Bitcoin, true, 0)?;
    for (field, value) in [
        ("to_subject_type", json!("address")),
        ("to_subject_id", json!(account(Network::Bitcoin, false))),
        ("transfer_type", json!("transfer")),
        ("output_index", json!(0)),
        ("event_sub_index", json!(1)),
        ("evidence_refs", json!([])),
        ("confidence", json!(0.5)),
        ("from_address", json!(account(Network::Bitcoin, true))),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        assert!(relationships::validate(&invalid).is_err(), "{field}");
    }
    let mut bad = row(Network::Bitcoin, false, 0)?;
    bad["output_index"] = json!(1);
    assert!(relationships::validate(&bad).is_err());
    Ok(())
}
#[test]
fn all_account_networks_share_typed_edges_and_exact_amounts() -> Result<()> {
    for network in [Network::Tron, Network::Ethereum, Network::Bsc] {
        let mut r = row(network, false, 0)?;
        r["amount_raw"] = json!(RawAmount::MAX);
        if network != Network::Tron {
            r["asset_id"] = json!(format!("erc1155:0x{}/{}", "a".repeat(40), RawAmount::MAX));
            r["token_id"] = json!(RawAmount::MAX);
            r["trace_address"] = json!([1, 0, 7]);
        }
        let edge = Edge::from_row(r)?;
        assert_eq!(edge.value()["amount"], RawAmount::MAX);
        assert_eq!(edge.value()["edge_semantics"], "observed_transfer");
        let mut graph = Graph::new(network);
        graph.edge(edge)?;
        assert_eq!(graph.value()["nodes"].as_array().unwrap().len(), 2);
    }
    let a = Subject::address(Network::Ethereum, &account(Network::Ethereum, true))?;
    let b = Subject::address(Network::Bsc, &account(Network::Bsc, true))?;
    assert_ne!(a.key(), b.key());
    assert!(
        Graph::new(Network::Ethereum)
            .merge(Graph::new(Network::Bsc), 5)
            .is_err()
    );
    Ok(())
}
#[test]
fn storage_and_deployed_script_identity_stay_immutable() -> Result<()> {
    assert_eq!(
        aml_warehouse::analytics_contract()?.checksum()?,
        "955e0c5ff2e6bb7000793e66408312f4d0172d026f15f328a9431ee050b568ec"
    );
    assert!(Subject::new(Network::Bitcoin, Kind::Address, "tx:not-an-address").is_err());
    assert!(relationships::script_subject_id("6A").is_err());
    assert!(
        Subject::new(
            Network::Ethereum,
            Kind::Script,
            &relationships::script_subject_id("6a")?
        )
        .is_err()
    );
    let zero = "410000000000000000000000000000000000000000";
    assert_eq!(
        aml_warehouse::values::tron_wallet(zero)?,
        "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"
    );
    assert_eq!(
        Subject::address(Network::Tron, zero)?.id(),
        "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"
    );
    assert!(Subject::new(Network::Tron, Kind::Address, zero).is_err());
    Ok(())
}
struct NoIo(AtomicUsize);
impl Connection for NoIo {
    async fn execute(&self, _: &str) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("unexpected IO")
    }
    async fn query(&self, _: &str) -> Result<Vec<String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("unexpected IO")
    }
}
#[tokio::test]
async fn invalid_limits_scope_and_database_fail_before_io() -> Result<()> {
    let db = NoIo(AtomicUsize::new(0));
    let root = Subject::address(Network::Ethereum, &account(Network::Ethereum, true))?;
    for query in [
        Query {
            max_transactions: 0,
            max_edges: 10,
        },
        Query {
            max_transactions: 101,
            max_edges: 10,
        },
        Query {
            max_transactions: 1,
            max_edges: 2001,
        },
    ] {
        assert!(
            relationships::read_neighborhood(&db, "fixture", Network::Ethereum, &root, query)
                .await
                .is_err()
        );
    }
    assert!(
        relationships::read_neighborhood(&db, "fixture", Network::Bsc, &root, Query::default())
            .await
            .is_err()
    );
    assert!(
        relationships::read_neighborhood(
            &db,
            "fixture; SELECT 1",
            Network::Ethereum,
            &root,
            Query::default()
        )
        .await
        .is_err()
    );
    assert_eq!(db.0.load(Ordering::SeqCst), 0);
    Ok(())
}
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
                    .context("String result required")
            })
            .collect()
    }
}
fn facts(network: Network) -> Result<(Block, BTreeMap<String, Vec<Value>>, Subject)> {
    let reference = row(network, true, 0)?;
    let mut b = fixture("aml_ingested_blocks", network)?;
    for key in [
        "block_number",
        "block_hash",
        "commit_id",
        "revision",
        "block_timestamp_unix_ms",
        "transaction_index",
    ] {
        if key != "transaction_index" {
            b[key] = reference[key].clone();
        }
    }
    b["canonical"] = json!(1);
    b["ingestion_status"] = json!("complete");
    b["transaction_count"] = json!(1);
    let block: Block = serde_json::from_value(b)?;
    let mut tx = fixture("aml_transactions", network)?;
    for key in [
        "block_number",
        "block_hash",
        "commit_id",
        "revision",
        "block_timestamp_unix_ms",
        "transaction_index",
        "tx_hash",
    ] {
        tx[key] = reference[key].clone();
    }
    if network == Network::Bitcoin {
        tx["input_count"] = json!(2);
        tx["output_count"] = json!(3);
        tx["coinbase"] = json!(0);
        tx["fee_raw"] = json!("1");
    }
    let mut tables = BTreeMap::from([
        ("aml_transactions".into(), vec![tx]),
        ("aml_address_relationships".into(), vec![]),
    ]);
    let count = if network == Network::Bitcoin { 5 } else { 1 };
    for position in 0..count {
        let input = position < 2;
        let i = if input { position } else { position - 2 };
        let mut r = if network == Network::Bitcoin {
            row(network, input, i)?
        } else {
            row(network, false, 0)?
        };
        if network != Network::Bitcoin {
            r["amount_raw"] = json!(RawAmount::MAX);
            r["transfer_type"] = json!("token_transfer");
            r["asset_id"] = json!(if network == Network::Tron {
                format!("trc20:41{}", "a".repeat(40))
            } else {
                format!("erc20:0x{}", "a".repeat(40))
            });
        }
        if network == Network::Bitcoin {
            let name = if input {
                "aml_bitcoin_inputs"
            } else {
                "aml_bitcoin_outputs"
            };
            let mut f = fixture(name, network)?;
            for key in [
                "block_number",
                "block_hash",
                "commit_id",
                "revision",
                "block_timestamp_unix_ms",
                "transaction_index",
                "tx_hash",
            ] {
                f[key] = r[key].clone();
            }
            f["address"] = Value::Null;
            f["script_hex"] = json!(format!("6a01{:02x}", i + u32::from(!input) * 16));
            f["value_sats"] = json!(r["amount_raw"].as_str().unwrap().parse::<u64>()?);
            if input {
                f["input_index"] = json!(i);
                f["prev_tx_hash"] = r["prev_tx_hash"].clone();
                f["prev_output_index"] = r["prev_output_index"].clone();
            } else {
                f["output_index"] = json!(i);
                f["coinbase"] = json!(0);
            }
            tables.entry(name.into()).or_insert_with(Vec::new).push(f);
        }
        tables.get_mut("aml_address_relationships").unwrap().push(r);
    }
    let root = if network == Network::Bitcoin {
        Subject::new(
            network,
            Kind::Script,
            reference["from_subject_id"].as_str().unwrap(),
        )?
    } else {
        Subject::address(network, &account(network, true))?
    };
    Ok((block, tables, root))
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn common_query_preserves_all_leg_evidence_and_scope_on_four_networks() -> Result<()> {
    let db = Http::new()?;
    for network in Network::ALL {
        let database = format!("db06_graph_{}", network.name());
        ensure!(
            db.query(&format!(
                "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
            ))
            .await?
                == ["0"],
            "fixture must be empty"
        );
        initialize(&db, &database, network).await?;
        let (block, tables, root) = facts(network)?;
        let empty =
            relationships::read_neighborhood(&db, &database, network, &root, Query::default())
                .await?;
        assert!(empty.graph.edges.is_empty());
        let previous = publication::state(&db, &database, network).await?;
        publication::reserve(&db, &database, &block).await?;
        for (table, rows) in &tables {
            publication::facts(&db, &database, &block, table, rows).await?;
        }
        assert!(
            relationships::read_neighborhood(&db, &database, network, &root, Query::default())
                .await?
                .graph
                .edges
                .is_empty()
        );
        core::publish_batch(&db, &database, &previous, &block, &tables, "{}").await?;
        let result =
            relationships::read_neighborhood(&db, &database, network, &root, Query::default())
                .await?;
        assert_eq!(
            result.graph.edges.len(),
            if network == Network::Bitcoin { 5 } else { 1 }
        );
        assert_eq!(result.selected_transactions, 1);
        assert_eq!(result.revision, 3);
        assert!(!result.graph.truncated);
        if network != Network::Bitcoin {
            assert_eq!(
                result.graph.edges.values().next().unwrap()["amount"],
                RawAmount::MAX
            );
        }
        if network == Network::Bitcoin {
            assert!(
                result.graph.value()["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|n| n["address"].is_null())
            );
            let clipped = relationships::read_neighborhood(
                &db,
                &database,
                network,
                &root,
                Query {
                    max_transactions: 1,
                    max_edges: 2,
                },
            )
            .await?;
            assert!(clipped.graph.truncated);
            assert_eq!(clipped.graph.edges.len(), 2);
            let mut bad = tables["aml_address_relationships"][0].clone();
            bad["amount_raw"] = json!("1");
            // A conflicting latest raw relationship remains auditable, but cannot
            // masquerade as a graph edge matching the original prevout value.
            bad["relationship_id"] = json!(format!(
                "{}:{}:in:99",
                bad["block_hash"].as_str().unwrap(),
                bad["tx_hash"].as_str().unwrap()
            ));
            bad["event_index"] = json!(99);
            publication::insert(&db, &database, "aml_address_relationships", &[bad]).await?;
            assert!(
                relationships::read_neighborhood(&db, &database, network, &root, Query::default())
                    .await
                    .is_err()
            );
            let mut witness = tables["aml_bitcoin_inputs"][0].clone();
            witness["input_index"] = json!(99);
            publication::insert(&db, &database, "aml_bitcoin_inputs", &[witness]).await?;
            assert!(
                relationships::read_neighborhood(&db, &database, network, &root, Query::default())
                    .await
                    .is_err(),
                "wrong amount must still fail after the input position exists"
            );
        }
    }
    Ok(())
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
            let mut state: Value = serde_json::from_str(&rows[0])?;
            state["revision"] = json!("999");
            rows[0] = state.to_string();
        }
        Ok(rows)
    }
}
#[tokio::test]
#[ignore = "requires an empty disposable ClickHouse 23.8"]
async fn repeated_transaction_hashes_keep_occurrences_separate_and_snapshot_changes_fail_closed()
-> Result<()> {
    let db = Http::new()?;
    let database = "db06_occurrences";
    let network = Network::Bitcoin;
    initialize(&db, database, network).await?;
    let (first, mut tables, _) = facts(network)?;
    tables.get_mut("aml_bitcoin_inputs").unwrap().clear();
    tables
        .get_mut("aml_address_relationships")
        .unwrap()
        .retain(|r| r["transfer_type"] == "utxo_output");
    let tx = &mut tables.get_mut("aml_transactions").unwrap()[0];
    tx["coinbase"] = json!(1);
    tx["input_count"] = json!(1);
    tx["fee_raw"] = json!("0");
    for output in tables.get_mut("aml_bitcoin_outputs").unwrap() {
        output["coinbase"] = json!(1);
    }
    let root = Subject::new(
        network,
        Kind::Script,
        tables["aml_address_relationships"][0]["to_subject_id"]
            .as_str()
            .unwrap(),
    )?;
    let initial = publication::state(&db, database, network).await?;
    let previous = core::publish_batch(&db, database, &initial, &first, &tables, "{}").await?;
    let mut second = first.clone();
    second.block_number += 1;
    second.parent_hash = first.block_hash.clone();
    second.block_hash = "d".repeat(64);
    second.revision = 6;
    second.commit_id = "next".into();
    second.block_timestamp_unix_ms += 1000;
    let header = serde_json::to_value(&second)?;
    for (table, rows) in &mut tables {
        for r in rows {
            for key in [
                "block_number",
                "block_hash",
                "commit_id",
                "revision",
                "block_timestamp_unix_ms",
            ] {
                r[key] = header[key].clone();
            }
            if table == "aml_address_relationships" {
                r["from_subject_id"] = json!(relationships::transaction_subject_id(
                    network,
                    &second.block_hash,
                    r["tx_hash"].as_str().unwrap()
                )?);
                r["relationship_id"] = json!(format!(
                    "{}:{}:out:{}",
                    second.block_hash,
                    r["tx_hash"].as_str().unwrap(),
                    r["event_index"]
                ));
            }
        }
    }
    core::publish_batch(&db, database, &previous, &second, &tables, "{}").await?;
    let one = relationships::read_neighborhood(
        &db,
        database,
        network,
        &root,
        Query {
            max_transactions: 1,
            max_edges: 50,
        },
    )
    .await?;
    assert_eq!(one.graph.edges.len(), 3);
    assert!(one.graph.truncated);
    assert!(one.graph.edges.values().all(|e| e["block_number"] == 8));
    let two = relationships::read_neighborhood(
        &db,
        database,
        network,
        &root,
        Query {
            max_transactions: 2,
            max_edges: 50,
        },
    )
    .await?;
    assert_eq!(two.graph.edges.len(), 6);
    assert_eq!(two.selected_transactions, 2);
    assert!(!two.graph.truncated);
    assert_eq!(
        two.graph
            .nodes
            .values()
            .filter(|n| n["node_type"] == "transaction")
            .count(),
        2
    );
    let moving = Moving {
        db: &db,
        reads: AtomicUsize::new(0),
    };
    let error =
        relationships::read_neighborhood(&moving, database, network, &root, Query::default())
            .await
            .unwrap_err();
    assert!(error.to_string().contains("checkpoint changed"));
    Ok(())
}
