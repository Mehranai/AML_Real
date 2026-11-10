//! Same paged read on every profile, using disposable ClickHouse and real publications.
use aml_warehouse::{
    Connection, Network, RawAmount, core, initialize,
    publication::{self, Block, State},
    relationships::{self, PageError, PageQuery, Subject, TransferDirection},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};
mod support;
use support::fixture;

struct Http {
    client: reqwest::Client,
    url: String,
}

#[tokio::test]
#[ignore = "requires a disposable WAREHOUSE_TEST_URL"]
async fn maintenance_scans_failures_and_verified_refresh_on_four_profiles() -> Result<()> {
    use aml_warehouse::maintenance::{self, Command};
    let db = Http {
        client: reqwest::Client::new(),
        url: std::env::var("WAREHOUSE_TEST_URL")?,
    };
    for network in Network::ALL {
        let database = format!("ing2_ops_{}", network.name());
        ensure!(
            db.query(&format!(
                "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
            ))
            .await?
                == ["0"],
            "maintenance fixture must be fresh"
        );
        initialize(&db, &database, network).await?;
        for height in [7, 8] {
            let (block, facts) = batch(network, height)?;
            let old = publication::state(&db, &database, network).await?;
            core::publish_batch(&db, &database, &old, &block, &facts, "{}").await?;
        }
        let healthy = maintenance::scan(&db, &database, network, 7, 8, 2, false).await?;
        assert_eq!(healthy["gaps"], 0);
        assert_eq!(healthy["source_snapshot"]["history_complete"], false);
        assert!(
            maintenance::scan(&db, &database, network, 0, 8, 9, false)
                .await
                .is_err()
        );
        for index in 0..3 {
            let mut row = fixture("aml_ingestion_failures", network)?;
            row["failure_id"] = json!(format!("fixture:{index}"));
            row["block_number"] = json!(7);
            row["revision"] = json!(30 + index);
            row["status"] = json!(if index == 2 { "dead_letter" } else { "retry" });
            row["attempts"] = json!(index + 1);
            publication::insert(&db, &database, "aml_ingestion_failures", &[row]).await?;
        }
        let mut query = Command::parse(&["failures".into(), "--limit".into(), "1".into()])?;
        let mut found = BTreeSet::new();
        loop {
            let page = maintenance::failures(&db, &database, network, &query).await?;
            for item in page["items"].as_array().unwrap() {
                assert!(found.insert(item["failure_id"].as_str().unwrap().to_owned()));
            }
            let Some(cursor) = page["pagination"]["next_cursor"].as_str() else {
                break;
            };
            query = Command::parse(&[
                "failures".into(),
                "--limit".into(),
                "1".into(),
                "--cursor".into(),
                cursor.into(),
            ])?;
        }
        assert_eq!(found.len(), 3);
        assert!(maintenance::blocked(&db, &database, network, 7).await?);
        let replay = Command::parse(&[
            "repair".into(),
            "--from-block".into(),
            "7".into(),
            "--to-block".into(),
            "7".into(),
            "--max-blocks".into(),
            "1".into(),
        ])?;
        assert!(
            maintenance::run(
                &db,
                &database,
                network,
                &replay,
                false,
                7,
                async |_, _| anyhow::bail!("must not write an unreviewed dead letter")
            )
            .await
            .is_err()
        );
        let old = publication::state(&db, &database, network).await?;
        let mut blocked_replay = replay.clone();
        blocked_replay.action = "replay".into();
        assert!(
            maintenance::run(
                &db,
                &database,
                network,
                &blocked_replay,
                false,
                7,
                async |_, _| anyhow::bail!("replay must not reopen a dead letter")
            )
            .await
            .is_err()
        );
        let mut reviewed = replay.clone();
        reviewed.include_dead = true;
        assert!(
            maintenance::run(
                &db,
                &database,
                network,
                &reviewed,
                false,
                7,
                async |_, _| anyhow::bail!("fixture unavailable RPC")
            )
            .await
            .is_err()
        );
        assert!(
            maintenance::blocked(&db, &database, network, 7).await?,
            "failed manual repair reopened an unverified dead letter"
        );
        db.execute(&format!("ALTER TABLE {database}.aml_transactions DELETE WHERE block_number=7 SETTINGS mutations_sync=2")).await?;
        assert_eq!(
            maintenance::scan(&db, &database, network, 7, 8, 2, false).await?["gaps"],
            1
        );
        assert!(
            maintenance::run(
                &db,
                &database,
                network,
                &reviewed,
                false,
                7,
                async |_, _| Ok(())
            )
            .await
            .is_err(),
            "an unverified write cannot resolve a gap"
        );
        assert!(maintenance::blocked(&db, &database, network, 7).await?);
        let mut repair = replay.clone();
        repair.include_dead = true;
        let result = maintenance::run(
            &db,
            &database,
            network,
            &repair,
            false,
            7,
            async |height, historical| {
                assert!(historical);
                let (mut block, mut facts) = batch(network, height)?;
                let current = publication::state(&db, &database, network).await?;
                block.revision =
                    publication::next_revision(&db, &database, network, current.revision).await?;
                block.commit_id = format!("repair-{}", block.revision);
                for rows in facts.values_mut() {
                    for row in rows {
                        row["revision"] = json!(block.revision);
                        row["commit_id"] = json!(block.commit_id);
                    }
                }
                core::refresh_observed_batch(&db, &database, &current, &block, &facts, "{}", 8)
                    .await?;
                Ok(())
            },
        )
        .await?;
        assert_eq!(result["status"], "complete");
        let current = publication::state(&db, &database, network).await?;
        assert_eq!(current.last_block_number, old.last_block_number);
        assert_eq!(current.last_block_hash, old.last_block_hash);
        assert!(current.revision > old.revision);
        assert!(!maintenance::blocked(&db, &database, network, 7).await?);
        assert_eq!(
            maintenance::scan(&db, &database, network, 7, 8, 2, false).await?["gaps"],
            0
        );
        let failures = maintenance::failures(
            &db,
            &database,
            network,
            &Command::parse(&["failures".into()])?,
        )
        .await?;
        assert!(
            failures["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["status"] == "resolved")
        );
        let mut outside = repair.clone();
        outside.from = Some(10);
        outside.to = Some(10);
        assert!(
            maintenance::run(
                &db,
                &database,
                network,
                &outside,
                false,
                7,
                async |_, _| anyhow::bail!("must not skip the frontier")
            )
            .await
            .is_err()
        );
        db.execute(&format!("ALTER TABLE {database}.aml_ingested_blocks DELETE WHERE block_number=7 SETTINGS mutations_sync=2")).await?;
        let marker_missing = maintenance::scan(&db, &database, network, 7, 8, 2, false).await?;
        assert!(marker_missing["gaps"].as_u64().unwrap() > 0);
        let error = maintenance::run(&db, &database, network, &repair, false, 7, async |_, _| {
            anyhow::bail!("must not rewind or publish without historical marker")
        })
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing historical block marker")
        );
        assert_eq!(publication::state(&db, &database, network).await?, current);
        eprintln!("ING-2 common maintenance: {} passed", network.name());
    }
    Ok(())
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
        ensure!(status.is_success(), "fixture query failed {status}: {body}");
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
            .filter(|line| !line.is_empty())
            .map(|line| {
                let value: Value = serde_json::from_str(line)?;
                value
                    .as_object()
                    .and_then(|v| v.values().next())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("String column required")
            })
            .collect()
    }
}
fn address(network: Network, other: bool) -> String {
    match network {
        Network::Bitcoin => if other {
            "1BitcoinEaterAddressDontSendf59kuE"
        } else {
            "1BoatSLRHtKNngkdXEeobR76b53LETtpyT"
        }
        .into(),
        Network::Ethereum | Network::Bsc => {
            format!("0x{}", if other { "2" } else { "1" }.repeat(40))
        }
        Network::Tron => aml_warehouse::values::tron_wallet(&format!(
            "41{}",
            if other { "2" } else { "1" }.repeat(40)
        ))
        .unwrap(),
    }
}
fn hash(network: Network, digit: char) -> String {
    aml_warehouse::values::normalize_hash(network, &digit.to_string().repeat(64)).unwrap()
}
fn script(address: &str) -> String {
    let mut bytes = [0u8; 25];
    for character in address.bytes() {
        let mut carry = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
            .iter()
            .position(|b| *b == character)
            .unwrap() as u32;
        for byte in bytes.iter_mut().rev() {
            carry += u32::from(*byte) * 58;
            *byte = carry as u8;
            carry >>= 8;
        }
        assert_eq!(carry, 0);
    }
    format!(
        "76a914{}88ac",
        bytes[1..21]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn batch(network: Network, height: u64) -> Result<(Block, BTreeMap<String, Vec<Value>>)> {
    let mut header = fixture("aml_ingested_blocks", network)?;
    header["block_number"] = json!(height);
    header["block_hash"] = json!(hash(network, if height == 7 { '7' } else { '8' }));
    header["parent_hash"] = json!(hash(network, if height == 7 { '6' } else { '7' }));
    header["block_timestamp_unix_ms"] = json!(height * 1000);
    header["commit_id"] = json!(format!("page-{height} ? '"));
    header["revision"] = json!(if height == 7 { 3 } else { 6 });
    header["canonical"] = json!(1);
    header["ingestion_status"] = json!("complete");
    header["transaction_count"] = json!(1);
    let block: Block = serde_json::from_value(header.clone())?;
    let common = |table: &str| -> Result<Value> {
        let mut row = fixture(table, network)?;
        for key in [
            "block_number",
            "block_hash",
            "block_timestamp_unix_ms",
            "commit_id",
            "revision",
        ] {
            row[key] = header[key].clone();
        }
        row["tx_hash"] = json!(hash(network, if height == 7 { 'a' } else { 'b' }));
        row["transaction_index"] = json!(0);
        Ok(row)
    };
    let mut tx = common("aml_transactions")?;
    let root = address(network, false);
    let other = address(network, true);
    if network == Network::Bitcoin {
        tx["coinbase"] = json!(u8::from(height == 7));
        tx["input_count"] = json!(1);
        tx["output_count"] = json!(5);
        tx["fee_raw"] = json!(if height == 7 { "0" } else { "1" });
    }
    let mut tables = BTreeMap::from([
        ("aml_transactions".into(), vec![tx]),
        ("aml_address_relationships".into(), vec![]),
    ]);
    for index in 0..5u32 {
        let mut row = common("aml_address_relationships")?;
        row["event_index"] = json!(index);
        row["event_sub_index"] = json!(0);
        row["confidence"] = json!(1.0);
        if network == Network::Bitcoin {
            let account = if index == 2 {
                Some(&other)
            } else if index == 4 {
                None
            } else {
                Some(&root)
            };
            let script_hex = account
                .map(|a| script(a))
                .unwrap_or_else(|| "6a01ff".into());
            let target = account
                .cloned()
                .unwrap_or(relationships::script_subject_id(&script_hex)?);
            let tx_hash = row["tx_hash"].as_str().unwrap().to_owned();
            row["relationship_id"] = json!(format!("{}:{tx_hash}:out:{index}", block.block_hash));
            row["from_subject_id"] = json!(relationships::transaction_subject_id(
                network,
                &block.block_hash,
                &tx_hash
            )?);
            row["from_subject_type"] = json!("transaction");
            row["from_address"] = Value::Null;
            row["to_subject_id"] = json!(target);
            row["to_subject_type"] = json!(if account.is_some() {
                "address"
            } else {
                "script"
            });
            row["to_address"] = json!(account);
            row["output_index"] = json!(index);
            row["transfer_type"] = json!("utxo_output");
            row["attribution_method"] = json!("observed_output");
            let amount = if height == 7 {
                (u64::from(index) + 1) * 10
            } else {
                [1, 2, 3, 1, 2][index as usize]
            };
            row["amount_raw"] = json!(amount.to_string());
            row["evidence_refs"] = json!([format!("outpoint:{tx_hash}:{index}")]);
            let mut output = common("aml_bitcoin_outputs")?;
            output["output_index"] = json!(index);
            output["address"] = json!(account);
            output["script_hex"] = json!(script_hex);
            output["value_sats"] = json!(amount);
            output["coinbase"] = json!(u8::from(height == 7));
            tables
                .entry("aml_bitcoin_outputs".into())
                .or_default()
                .push(output);
        } else {
            let incoming = index == 2;
            row["from_subject_id"] = json!(if incoming { &other } else { &root });
            row["to_subject_id"] = json!(if incoming || index == 3 {
                &root
            } else {
                &other
            });
            row["from_address"] = row["from_subject_id"].clone();
            row["to_address"] = row["to_subject_id"].clone();
            row["relationship_id"] = json!(format!("event-{height}-{index}"));
            if index == 1 {
                row["asset_id"] = json!(if network == Network::Tron {
                    format!("trc20:41{}", "a".repeat(40))
                } else {
                    format!("erc20:0x{}", "a".repeat(40))
                });
                row["amount_raw"] = json!(RawAmount::MAX);
                row["transfer_type"] = json!("token_transfer");
            }
            if index == 2 || index == 4 {
                let id = if index == 2 { RawAmount::MAX } else { "7" };
                row["asset_id"] = json!(if network == Network::Tron {
                    format!("trc721:41{}/{id}", "c".repeat(40))
                } else {
                    format!("erc1155:0x{}/{id}", "c".repeat(40))
                });
                row["token_id"] = json!(id);
                row["amount_raw"] = json!("1");
                row["transfer_type"] = json!("token_transfer");
                row["decimals"] = Value::Null;
                if network != Network::Tron {
                    row["event_index"] = json!(2);
                    row["event_sub_index"] = json!(u32::from(index == 4));
                }
            }
        }
        tables
            .get_mut("aml_address_relationships")
            .unwrap()
            .push(row);
    }
    if network == Network::Bitcoin && height == 8 {
        let mut input = common("aml_bitcoin_inputs")?;
        input["input_index"] = json!(0);
        input["prev_tx_hash"] = json!(hash(network, 'a'));
        input["prev_output_index"] = json!(0);
        input["address"] = json!(root);
        input["script_hex"] = json!(script(&root));
        input["value_sats"] = json!(10);
        let mut row = common("aml_address_relationships")?;
        let tx_hash = row["tx_hash"].as_str().unwrap().to_owned();
        row["relationship_id"] = json!(format!("{}:{tx_hash}:in:0", block.block_hash));
        row["from_subject_id"] = json!(root);
        row["from_subject_type"] = json!("address");
        row["from_address"] = json!(root);
        row["to_subject_id"] = json!(relationships::transaction_subject_id(
            network,
            &block.block_hash,
            &tx_hash
        )?);
        row["to_subject_type"] = json!("transaction");
        row["to_address"] = Value::Null;
        row["transfer_type"] = json!("utxo_input");
        row["attribution_method"] = json!("observed_prevout");
        row["event_index"] = json!(0);
        row["output_index"] = Value::Null;
        row["prev_tx_hash"] = input["prev_tx_hash"].clone();
        row["prev_output_index"] = json!(0);
        row["amount_raw"] = json!("10");
        row["confidence"] = json!(1.0);
        row["evidence_refs"] = json!([format!("outpoint:{}:0", hash(network, 'a'))]);
        tables.insert("aml_bitcoin_inputs".into(), vec![input]);
        tables
            .get_mut("aml_address_relationships")
            .unwrap()
            .push(row);
    }
    Ok((block, tables))
}

struct ChangeAfterSelect<'a> {
    db: &'a Http,
    database: String,
    network: Network,
    changed: AtomicBool,
}
impl Connection for ChangeAfterSelect<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.db.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.db.query(sql).await
    }
    async fn query_bounded(&self, sql: &str) -> Result<Vec<String>> {
        let result = self.db.query(sql).await?;
        if sql.contains("event_sub_index)) FROM") && !self.changed.swap(true, Ordering::SeqCst) {
            let before = publication::state(self.db, &self.database, self.network).await?;
            let after = State {
                revision: before.revision + 1,
                observed_rpc_height: before.observed_rpc_height + 1,
                ..before.clone()
            };
            publication::publish(self.db, &self.database, self.network, &before, &after).await?;
        }
        Ok(result)
    }
}

#[tokio::test]
#[ignore = "requires a fresh disposable WAREHOUSE_TEST_URL"]
async fn filtered_keyset_pages_and_snapshot_conflicts_on_four_profiles() -> Result<()> {
    let db = Http {
        client: reqwest::Client::new(),
        url: std::env::var("WAREHOUSE_TEST_URL")?,
    };
    for network in Network::ALL {
        let database = format!("api2_pages_{}", network.name());
        ensure!(
            db.query(&format!(
                "SELECT toString(count()) FROM system.tables WHERE database='{database}'"
            ))
            .await?
                == ["0"],
            "fixture must be fresh"
        );
        initialize(&db, &database, network).await?;
        let root = Subject::address(network, &address(network, false))?;
        let all = PageQuery {
            limit: 100,
            ..Default::default()
        };
        let empty = relationships::read_page(&db, &database, network, &root, &all).await?;
        assert!(empty["items"].as_array().unwrap().is_empty());
        assert_eq!(empty["history_complete"], false);
        for height in [7, 8] {
            let (block, facts) = batch(network, height)?;
            let before = publication::state(&db, &database, network).await?;
            core::publish_batch(&db, &database, &before, &block, &facts, "{}").await?;
        }
        let full = relationships::read_page(&db, &database, network, &root, &all).await?;
        assert_eq!(
            full["items"].as_array().unwrap().len(),
            if network == Network::Bitcoin { 7 } else { 10 }
        );
        let mut query = PageQuery {
            limit: 2,
            ..Default::default()
        };
        let mut items = vec![];
        let mut seen = BTreeSet::new();
        let first = relationships::read_page(&db, &database, network, &root, &query).await?;
        let first_cursor = first["pagination"]["next_cursor"]
            .as_str()
            .unwrap()
            .to_owned();
        for _ in 0..10 {
            let page = relationships::read_page(&db, &database, network, &root, &query).await?;
            assert_eq!(page["source_snapshot"], full["source_snapshot"]);
            for row in page["items"].as_array().unwrap() {
                assert!(seen.insert(row["id"].as_str().unwrap().to_owned()));
                items.push(row.clone());
            }
            query.cursor = page["pagination"]["next_cursor"]
                .as_str()
                .map(str::to_owned);
            if query.cursor.is_none() {
                break;
            }
        }
        assert_eq!(Value::Array(items), full["items"]);
        let next = PageQuery {
            limit: 2,
            cursor: Some(first_cursor.clone()),
            ..Default::default()
        };
        assert_eq!(
            relationships::read_page(&db, &database, network, &root, &next).await?["items"],
            relationships::read_page(&db, &database, network, &root, &next).await?["items"]
        );
        for (filter, expected) in [
            (
                PageQuery {
                    from_block: Some(7),
                    to_block: Some(7),
                    ..all.clone()
                },
                if network == Network::Bitcoin { 3 } else { 5 },
            ),
            (
                PageQuery {
                    from_unix_ms: Some(8000),
                    to_unix_ms: Some(8000),
                    ..all.clone()
                },
                if network == Network::Bitcoin { 4 } else { 5 },
            ),
            (
                PageQuery {
                    to_unix_ms: Some(6999),
                    ..all.clone()
                },
                0,
            ),
            (
                PageQuery {
                    asset_id: Some(network.native_asset().into()),
                    ..all.clone()
                },
                if network == Network::Bitcoin { 7 } else { 4 },
            ),
            (
                PageQuery {
                    direction: TransferDirection::Incoming,
                    ..all.clone()
                },
                if network == Network::Bitcoin { 6 } else { 4 },
            ),
            (
                PageQuery {
                    direction: TransferDirection::Outgoing,
                    ..all.clone()
                },
                if network == Network::Bitcoin { 1 } else { 8 },
            ),
        ] {
            let value = relationships::read_page(&db, &database, network, &root, &filter).await?;
            assert_eq!(
                value["items"].as_array().unwrap().len(),
                expected,
                "{} {:?}",
                network.name(),
                filter
            );
        }
        if network != Network::Bitcoin {
            let collection = if network == Network::Tron {
                format!("trc721:41{}", "c".repeat(40))
            } else {
                format!("erc1155:0x{}", "c".repeat(40))
            };
            let page = relationships::read_page(
                &db,
                &database,
                network,
                &root,
                &PageQuery {
                    asset_id: Some(collection.clone()),
                    ..all.clone()
                },
            )
            .await?;
            assert_eq!(page["items"].as_array().unwrap().len(), 4);
            let token = relationships::read_page(
                &db,
                &database,
                network,
                &root,
                &PageQuery {
                    asset_id: Some(format!("{collection}/{}", RawAmount::MAX)),
                    ..all.clone()
                },
            )
            .await?;
            assert_eq!(token["items"].as_array().unwrap().len(), 2);
            assert!(
                token["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["token_id"] == RawAmount::MAX)
            );
            assert!(
                full["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["amount"] == RawAmount::MAX)
            );
        }
        let race = ChangeAfterSelect {
            db: &db,
            database: database.clone(),
            network,
            changed: AtomicBool::new(false),
        };
        assert!(matches!(
            relationships::read_page(&race, &database, network, &root, &all).await,
            Err(PageError::SourceChanged)
        ));
        assert!(race.changed.load(Ordering::SeqCst));
        assert!(matches!(
            relationships::read_page(&db, &database, network, &root, &next).await,
            Err(PageError::SourceChanged)
        ));
        let fresh = relationships::read_page(
            &db,
            &database,
            network,
            &root,
            &PageQuery {
                limit: 2,
                ..Default::default()
            },
        )
        .await?;
        assert_ne!(fresh["source_snapshot"], full["source_snapshot"]);
        assert_eq!(fresh["items"], first["items"]);
        if network == Network::Bitcoin {
            assert_eq!(full["model"], "address_transaction_utxo");
            assert!(
                full["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["from_subject_type"] == "transaction"
                        || r["to_subject_type"] == "transaction")
            );
            assert_eq!(full["amount_allocation_claimed"], false);
        }
        println!(
            "{}: exact pages, filters, witnesses and snapshot conflicts passed",
            network.name()
        );
    }
    Ok(())
}
