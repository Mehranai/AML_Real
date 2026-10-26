use axum::{Json, Router, extract::State, routing::post};
use bitcoin_aml::{
    GENESIS,
    config::IngestConfig,
    probe::{self, Readiness},
    rpc::Rpc,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
struct Fixture {
    info: Value,
    inline: bool,
    raw_available: bool,
    index: Option<Value>,
    reject_v3: bool,
    wrong_genesis: bool,
    broken_block: bool,
    conflicting_raw: bool,
    malformed_inline: bool,
    coinbase_only: bool,
    input_count: usize,
    reorg: bool,
    rpc_failure: bool,
    same_block_spend: bool,
    stall_ms: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn hash(height: u64) -> String {
    if height == 0 {
        GENESIS.into()
    } else {
        format!("{height:064x}")
    }
}
fn output(value: &str) -> Value {
    json!({"n":0,"value":value,"scriptPubKey":{"hex":"51","type":"nonstandard"}})
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            info: json!({"chain":"main","blocks":1200,"headers":1200,"bestblockhash":hash(1200),"time":now(),"verificationprogress":1.0,"initialblockdownload":false,"pruned":false}),
            inline: true,
            raw_available: false,
            index: Some(json!({})),
            reject_v3: false,
            wrong_genesis: false,
            broken_block: false,
            conflicting_raw: false,
            malformed_inline: false,
            coinbase_only: false,
            input_count: 1,
            reorg: false,
            rpc_failure: false,
            same_block_spend: false,
            stall_ms: 0,
        }
    }
}
struct MockState {
    fixture: Fixture,
    requests: Mutex<Vec<Value>>,
    hash_reads: Mutex<HashMap<u64, usize>>,
}
struct Mock {
    url: String,
    state: Arc<MockState>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Mock {
    async fn start(fixture: Fixture) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(MockState {
            fixture,
            requests: Mutex::new(Vec::new()),
            hash_reads: Mutex::new(HashMap::new()),
        });
        let app = Router::new()
            .route("/", post(reply))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, state, server }
    }
    async fn inspect(&self, config: &IngestConfig) -> probe::Report {
        let rpc = Rpc::new(&self.url, "fixture-user".into(), "fixture-secret".into(), 2).unwrap();
        probe::inspect(&rpc, config).await
    }
    fn requests(&self) -> Vec<Value> {
        self.state.requests.lock().unwrap().clone()
    }
}
fn config() -> IngestConfig {
    IngestConfig {
        start: 1000,
        end: Some(1100),
        confirmations: 1,
        reorg_depth: 144,
        poll_seconds: 10,
        verbosity: 3,
    }
}
async fn reply(State(state): State<Arc<MockState>>, Json(request): Json<Value>) -> Json<Value> {
    state.requests.lock().unwrap().push(request.clone());
    let f = &state.fixture;
    if f.stall_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(f.stall_ms)).await;
    }
    let failure = || {
        Json(
            json!({"id":request["id"],"result":null,"error":{"code":-5,"message":"fixture-secret: private provider response"}}),
        )
    };
    if f.rpc_failure {
        return failure();
    }
    let value = match request["method"].as_str().unwrap() {
        "getblockchaininfo" => f.info.clone(),
        "getindexinfo" => match &f.index {
            Some(v) => v.clone(),
            None => return failure(),
        },
        "getblockhash" => {
            let h = request["params"][0].as_u64().unwrap();
            let mut reads = state.hash_reads.lock().unwrap();
            let count = reads.entry(h).or_default();
            *count += 1;
            if (h == 0 && f.wrong_genesis) || (h == 1000 && f.reorg && *count > 1) {
                json!("e".repeat(64))
            } else {
                json!(hash(h))
            }
        }
        "getblock" => {
            let verbosity = request["params"][1].as_u64().unwrap();
            if verbosity == 3 && f.reject_v3 {
                return failure();
            }
            let blockhash = request["params"][0].as_str().unwrap();
            let h = if blockhash == GENESIS {
                0
            } else {
                u64::from_str_radix(blockhash, 16).unwrap()
            };
            let mut txs = vec![
                json!({"txid":format!("f{h:063x}"),"vin":[{"coinbase":"00"}],"vout":[output("50")]}),
            ];
            if h != 0 && !f.coinbase_only {
                let vin: Vec<Value> = (0..f.input_count)
                    .map(|i| {
                        let mut input = json!({"txid":format!("b{:063x}",h + i as u64),"vout":0});
                        if f.inline && verbosity == 3 {
                            input["prevout"] = if f.malformed_inline {
                                json!({"value":"1"})
                            } else {
                                output("1")
                            };
                        }
                        input
                    })
                    .collect();
                txs.push(json!({"txid":format!("a{h:063x}"),"vin":vin,"vout":[output("0.9")]}));
                if f.same_block_spend {
                    txs.push(json!({"txid":format!("c{h:063x}"),"vin":[{"txid":format!("a{h:063x}"),"vout":0}],"vout":[output("0.8")]}));
                }
            }
            json!({"hash":if f.broken_block { "0".repeat(64) } else { blockhash.into() },"height":h,"confirmations":10,"tx":txs})
        }
        "getrawtransaction" if f.raw_available => {
            json!({"txid":request["params"][0],"vout":[output(if f.conflicting_raw { "2" } else { "1" })]})
        }
        _ => return failure(),
    };
    Json(json!({"id":request["id"],"result":value,"error":null}))
}
fn has(report: &probe::Report, code: &str) -> bool {
    report.checks.iter().any(|c| c.code == code)
}

#[tokio::test]
async fn earlier_outputs_in_the_same_block_resolve_without_extra_rpc() {
    let mock = Mock::start(Fixture {
        same_block_spend: true,
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Ready);
    for sample in &report.samples {
        assert_eq!(sample.inputs_sampled, 2);
        assert_eq!(sample.in_block_prevouts, 1);
        assert_eq!(sample.raw_transaction_lookups, 1);
        assert_eq!(sample.unresolved_inputs, 0);
    }
}

#[tokio::test]
async fn stalled_rpc_times_out_and_invalid_library_config_does_not_call_rpc() {
    let mock = Mock::start(Fixture {
        stall_ms: 2000,
        ..Fixture::default()
    })
    .await;
    let rpc = Rpc::new(&mock.url, "fixture".into(), "fixture-secret".into(), 1).unwrap();
    let report = probe::inspect(&rpc, &config()).await;
    assert_eq!(report.status, Readiness::Unready);
    assert!(has(&report, "rpc_unavailable"));
    let before = mock.requests().len();
    let report = probe::inspect(
        &rpc,
        &IngestConfig {
            confirmations: 0,
            ..config()
        },
    )
    .await;
    assert_eq!(report.status, Readiness::Unready);
    assert!(has(&report, "configuration_invalid"));
    assert_eq!(mock.requests().len(), before);
}

#[tokio::test]
async fn undo_ready_without_txindex_and_only_read_only_rpc_methods() {
    let mock = Mock::start(Fixture::default()).await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Ready);
    assert_eq!(report.exit_code(), 0);
    assert_eq!(report.samples.len(), 2);
    assert_eq!(report.samples[0].inline_prevouts, 1);
    assert_eq!(report.txindex.enabled, Some(false));
    assert!(has(&report, "raw_fallback_unavailable"));
    for request in mock.requests() {
        assert!(matches!(
            request["method"].as_str().unwrap(),
            "getblockchaininfo"
                | "getblockhash"
                | "getindexinfo"
                | "getblock"
                | "getrawtransaction"
        ));
    }
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["scope"]["local_database_checked"], false);
    assert_eq!(value["scope"]["full_history_verified"], false);
    assert!(!value.to_string().contains("fixture-secret"));
    assert!(!value.to_string().contains(&mock.url));
}

#[tokio::test]
async fn verbosity_two_uses_real_raw_lookup_and_synced_index() {
    let f = Fixture {
        inline: false,
        raw_available: true,
        index: Some(json!({"txindex":{"synced":true,"best_block_height":1200}})),
        ..Fixture::default()
    };
    let mock = Mock::start(f).await;
    let c = IngestConfig {
        verbosity: 2,
        ..config()
    };
    let report = mock.inspect(&c).await;
    assert_eq!(report.status, Readiness::Ready);
    assert_eq!(report.samples[0].inline_prevouts, 0);
    assert_eq!(report.samples[0].raw_fallback_verified, 1);
}

#[tokio::test]
async fn missing_prevouts_distinguish_fresh_partial_start_from_genesis_replay() {
    let mock = Mock::start(Fixture {
        inline: false,
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Unready);
    assert!(has(&report, "cold_start_prevouts_unavailable"));
    let report = mock
        .inspect(&IngestConfig {
            start: 0,
            ..config()
        })
        .await;
    assert_eq!(report.status, Readiness::Degraded);
    assert_eq!(report.exit_code(), 2);
    assert!(has(&report, "local_history_required"));
}

#[tokio::test]
async fn wrong_chain_or_genesis_never_reads_transaction_data() {
    for wrong_chain in [true, false] {
        let mut f = Fixture::default();
        if wrong_chain {
            f.info["chain"] = json!("signet");
        } else {
            f.wrong_genesis = true;
        }
        let mock = Mock::start(f).await;
        let report = mock.inspect(&config()).await;
        assert_eq!(report.status, Readiness::Unready);
        assert!(has(
            &report,
            if wrong_chain {
                "wrong_network"
            } else {
                "genesis_mismatch"
            }
        ));
        assert!(!mock.requests().iter().any(|r| r["method"] == "getblock"));
    }
}

#[tokio::test]
async fn prune_boundary_is_enforced_and_retained_history_is_degraded() {
    for floor in [900, 1050] {
        let mut f = Fixture::default();
        f.info["pruned"] = json!(true);
        f.info["pruneheight"] = json!(floor);
        let mock = Mock::start(f).await;
        let report = mock.inspect(&config()).await;
        if floor > 1000 {
            assert_eq!(report.status, Readiness::Unready);
            assert!(has(&report, "requested_history_pruned"));
            assert!(report.samples.is_empty());
        } else {
            assert_eq!(report.status, Readiness::Degraded);
            assert!(has(&report, "pruned_history"));
            assert_eq!(report.samples.len(), 2);
        }
    }
}

#[tokio::test]
async fn syncing_stale_tip_and_no_confirmed_range_are_explicit() {
    let mut f = Fixture::default();
    f.info["headers"] = json!(1250);
    f.info["initialblockdownload"] = json!(true);
    f.info["time"] = json!(now() - 10000);
    let mock = Mock::start(f).await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Degraded);
    assert!(has(&report, "node_syncing") && has(&report, "node_tip_time"));
    let report = mock
        .inspect(&IngestConfig {
            start: 1201,
            end: None,
            ..config()
        })
        .await;
    assert_eq!(report.status, Readiness::Unready);
    assert!(has(&report, "no_confirmed_range"));
}

#[tokio::test]
async fn index_claim_does_not_substitute_for_an_actual_prevout_lookup() {
    let f = Fixture {
        inline: false,
        index: Some(json!({"txindex":{"synced":true,"best_block_height":1200}})),
        ..Fixture::default()
    };
    let mock = Mock::start(f).await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Unready);
    assert!(report.samples.iter().all(|s| s.unresolved_inputs > 0));
    let f = Fixture {
        inline: false,
        raw_available: true,
        index: Some(json!({"txindex":{"synced":false,"best_block_height":100}})),
        ..Fixture::default()
    };
    let mock = Mock::start(f).await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Degraded);
    assert!(has(&report, "fallback_index_unconfirmed"));
}

#[tokio::test]
async fn optional_index_rpc_can_be_unavailable_when_undo_is_usable() {
    let mock = Mock::start(Fixture {
        index: None,
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Ready);
    assert!(!report.txindex.metadata_available);
    assert_eq!(report.txindex.enabled, None);
    assert!(has(&report, "txindex_metadata_unavailable"));
}

#[tokio::test]
async fn configured_verbosity_is_not_silently_changed_when_three_is_unsupported() {
    let mock = Mock::start(Fixture {
        reject_v3: true,
        raw_available: true,
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Unready);
    assert_eq!(report.suggested_block_verbosity, Some(2));
    assert!(has(&report, "configured_verbosity_unavailable"));
    assert!(report.samples.iter().all(|s| s.verbosity == 2));
    assert_eq!(report.scope.as_ref().unwrap().configured_verbosity, 3);
}

#[tokio::test]
async fn malformed_and_conflicting_rpc_evidence_never_reports_ready() {
    let mut fixtures = vec![
        Fixture {
            broken_block: true,
            ..Fixture::default()
        },
        Fixture {
            malformed_inline: true,
            ..Fixture::default()
        },
        Fixture {
            raw_available: true,
            conflicting_raw: true,
            ..Fixture::default()
        },
    ];
    let mut missing = Fixture::default();
    missing.info.as_object_mut().unwrap().remove("headers");
    fixtures.push(missing);
    let mut missing_prune = Fixture::default();
    missing_prune.info["pruned"] = json!(true);
    fixtures.push(missing_prune);
    for fixture in fixtures {
        let mock = Mock::start(fixture).await;
        let report = mock.inspect(&config()).await;
        assert_eq!(report.status, Readiness::Unready);
        assert!(has(&report, "invalid_rpc_evidence"));
    }
}

#[tokio::test]
async fn coinbase_only_sample_is_not_proof_of_prevout_capability() {
    let mock = Mock::start(Fixture {
        coinbase_only: true,
        ..Fixture::default()
    })
    .await;
    let report = mock
        .inspect(&IngestConfig {
            start: 1000,
            end: Some(1000),
            ..config()
        })
        .await;
    assert_eq!(report.status, Readiness::Degraded);
    assert_eq!(report.samples.len(), 1);
    assert!(has(&report, "external_prevouts_not_tested"));
    assert!(
        !mock
            .requests()
            .iter()
            .any(|r| r["method"] == "getrawtransaction")
    );
}

#[tokio::test]
async fn large_blocks_have_bounded_prevout_sampling_and_rpc_work() {
    let mock = Mock::start(Fixture {
        inline: false,
        raw_available: true,
        input_count: 64,
        index: Some(json!({"txindex":{"synced":true,"best_block_height":1200}})),
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Ready);
    for sample in &report.samples {
        assert_eq!(sample.non_coinbase_inputs, 64);
        assert_eq!(sample.inputs_sampled, 8);
        assert_eq!(sample.raw_transaction_lookups, 8);
    }
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|r| r["method"] == "getrawtransaction")
            .count(),
        16
    );
}

#[tokio::test]
async fn reorg_during_probe_and_rpc_failure_are_reported_without_private_text() {
    let mock = Mock::start(Fixture {
        reorg: true,
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Degraded);
    assert!(has(&report, "chain_changed_during_probe"));
    let mock = Mock::start(Fixture {
        rpc_failure: true,
        ..Fixture::default()
    })
    .await;
    let report = mock.inspect(&config()).await;
    assert_eq!(report.status, Readiness::Unready);
    assert!(has(&report, "rpc_unavailable"));
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("fixture-secret")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_cli_needs_no_database_and_propagates_all_three_exit_codes() {
    let dir = std::env::temp_dir().join(format!("bitcoin-probe-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (fixture, code, status) in [
        (Fixture::default(), 0, "ready"),
        (
            Fixture {
                coinbase_only: true,
                ..Fixture::default()
            },
            2,
            "degraded",
        ),
        (
            Fixture {
                wrong_genesis: true,
                ..Fixture::default()
            },
            1,
            "unready",
        ),
    ] {
        let mock = Mock::start(fixture).await;
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_bitcoin_aml"))
            .current_dir(&dir)
            .env_clear()
            .env("BITCOIN_RPC_URL", &mock.url)
            .env("BITCOIN_RPC_USER", "fixture-user")
            .env("BITCOIN_RPC_PASSWORD", "fixture-secret")
            .env("BITCOIN_START_BLOCK", "1000")
            .env("BITCOIN_END_BLOCK", "1100")
            .env("BITCOIN_CONFIRMATIONS", "1")
            .args(["probe", "--compact"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], status);
        assert!(output.stderr.is_empty());
    }
    for args in [
        vec!["probe", "--help"],
        vec!["probe"],
        vec!["probe", "--unknown"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_bitcoin_aml"))
            .current_dir(&dir)
            .env_clear()
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), args.contains(&"--help"));
        if args == ["probe"] {
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["checks"][0]["code"], "configuration_invalid");
        }
    }
    std::fs::remove_dir(&dir).unwrap();
}
