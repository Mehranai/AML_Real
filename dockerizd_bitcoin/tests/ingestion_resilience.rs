use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use bitcoin_aml::{
    GENESIS,
    config::{DatabaseConfig, IngestConfig, PipelineConfig, RpcPolicy},
    db::Database,
    failures, fetch, ingestion, repair, resolver,
    rpc::{Rpc, RpcPool},
    warehouse,
    writer_lock::WriterLock,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn hash(h: u64) -> String {
    if h == 0 {
        GENESIS.into()
    } else {
        format!("{h:064x}")
    }
}
fn output(n: u64, value: &str) -> Value {
    json!({"n":n,"value":value,"scriptPubKey":{"hex":"76a914111111111111111111111111111111111111111188ac","type":"pubkeyhash"}})
}
fn block(h: u64) -> Value {
    let mut txs = vec![
        json!({"txid":format!("{:064x}",100+h),"vin":[{"coinbase":"00"}],"vout":[output(0,"50")]}),
    ];
    if h == 1 || h == 2 {
        txs.push(json!({"txid":format!("{:064x}",9000+h),"vin":[{"txid":format!("{:064x}",if h==1 {100} else {9001}),"vout":0}],"vout":[output(0,if h==1 {"49"} else {"48"})]}));
    }
    if h == 3 {
        txs.push(json!({"txid":format!("{:064x}",9003),"vin":[{"txid":"a".repeat(64),"vout":0},{"txid":"a".repeat(64),"vout":1},{"txid":"b".repeat(64),"vout":0}],"vout":[output(0,"8.99999999")]}));
    }
    json!({"height":h,"hash":hash(h),"previousblockhash":if h==0 {String::new()} else {hash(h-1)},"time":1231006505+h,"tx":txs})
}
#[derive(Clone)]
struct Settings {
    blocks: Vec<Value>,
    chain: &'static str,
    http_status: u16,
    fail_first: usize,
    rpc_error: Option<i64>,
    batch_fault: u8,
    slow_first: bool,
    missing_raw: bool,
    broken_window: bool,
    huge: bool,
    raw_script_bytes: usize,
    result_status: u16,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            blocks: (0..4).map(block).collect(),
            chain: "main",
            http_status: 200,
            fail_first: 0,
            rpc_error: None,
            batch_fault: 0,
            slow_first: false,
            missing_raw: false,
            broken_window: false,
            huge: false,
            raw_script_bytes: 0,
            result_status: 200,
        }
    }
}
struct Server {
    settings: RwLock<Settings>,
    requests: Mutex<Vec<Value>>,
    count: AtomicUsize,
    inflight: AtomicUsize,
    max_inflight: AtomicUsize,
    completion: Mutex<Vec<u64>>,
}
struct Mock {
    url: String,
    state: Arc<Server>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Mock {
    async fn start(settings: Settings) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Server {
            settings: RwLock::new(settings),
            requests: Mutex::new(vec![]),
            count: AtomicUsize::new(0),
            inflight: AtomicUsize::new(0),
            max_inflight: AtomicUsize::new(0),
            completion: Mutex::new(vec![]),
        });
        let app = Router::new()
            .route("/", post(reply))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, state, task }
    }
    fn rpc(&self) -> Rpc {
        Rpc::new(
            &self.url,
            "fixture".into(),
            "private-fixture-secret".into(),
            2,
        )
        .unwrap()
    }
    fn requests(&self) -> Vec<Value> {
        self.state.requests.lock().unwrap().clone()
    }
    fn calls(&self, method: &str) -> Vec<Value> {
        self.requests()
            .into_iter()
            .flat_map(|r| r.as_array().cloned().unwrap_or_else(|| vec![r]))
            .filter(|r| r["method"] == method)
            .collect()
    }
}
fn envelope(settings: &Settings, request: &Value) -> Value {
    let error = || json!({"id":request["id"],"result":null,"error":{"code":settings.rpc_error.unwrap_or(-5),"message":"private-fixture-secret"}});
    if settings.rpc_error.is_some() {
        return error();
    }
    let result = match request["method"].as_str().unwrap() {
        "getblockchaininfo" => json!({"chain":settings.chain}),
        "getblockcount" => json!(settings.blocks.len() - 1),
        "getblockhash" => match settings
            .blocks
            .get(request["params"][0].as_u64().unwrap() as usize)
        {
            Some(b) => b["hash"].clone(),
            None => return error(),
        },
        "getblock" => match settings
            .blocks
            .iter()
            .find(|b| b["hash"] == request["params"][0])
        {
            Some(b) => {
                let mut b = b.clone();
                if settings.broken_window && b["height"] == 1 {
                    b["previousblockhash"] = json!("f".repeat(64));
                }
                b
            }
            None => return error(),
        },
        "getrawtransaction" if !settings.missing_raw => {
            let id = request["params"][0].as_str().unwrap();
            let mut vout = if id == "a".repeat(64) {
                vec![output(0, "2"), output(1, "3")]
            } else {
                vec![output(0, "4")]
            };
            if settings.raw_script_bytes > 0 {
                for output in &mut vout {
                    output["scriptPubKey"]["hex"] = json!("51".repeat(settings.raw_script_bytes));
                }
            }
            json!({"txid":id,"vout":vout})
        }
        _ => return error(),
    };
    json!({"id":request["id"],"result":result,"error":null})
}
async fn reply(State(state): State<Arc<Server>>, Json(request): Json<Value>) -> Response {
    state.requests.lock().unwrap().push(request.clone());
    let number = state.count.fetch_add(1, Ordering::SeqCst) + 1;
    let settings = state.settings.read().unwrap().clone();
    if number <= settings.fail_first {
        if let Some(code) = settings.rpc_error {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"id":request["id"],"result":null,"error":{"code":code}})),
            )
                .into_response();
        }
        return (
            StatusCode::from_u16(settings.http_status).unwrap(),
            "temporary private-fixture-secret",
        )
            .into_response();
    }
    if settings.huge {
        return Json(json!({"id":request["id"],"result":"z".repeat(4096),"error":null}))
            .into_response();
    }
    if request["method"] == "getblock" {
        let current = state.inflight.fetch_add(1, Ordering::SeqCst) + 1;
        state.max_inflight.fetch_max(current, Ordering::SeqCst);
        let h = settings
            .blocks
            .iter()
            .find(|b| b["hash"] == request["params"][0])
            .unwrap()["height"]
            .as_u64()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(if settings.slow_first && h == 0 {
            60
        } else {
            5
        }))
        .await;
        state.completion.lock().unwrap().push(h);
        state.inflight.fetch_sub(1, Ordering::SeqCst);
    }
    let mut value = if let Some(calls) = request.as_array() {
        Value::Array(calls.iter().rev().map(|r| envelope(&settings, r)).collect())
    } else {
        envelope(&settings, &request)
    };
    if let Some(rows) = value.as_array_mut() {
        match settings.batch_fault {
            1 => {
                rows[0]["id"] = json!(u64::MAX);
            }
            2 => {
                rows[0] = rows[1].clone();
            }
            3 => {
                rows.pop();
            }
            _ => {}
        }
    }
    (
        StatusCode::from_u16(settings.result_status).unwrap(),
        Json(value),
    )
        .into_response()
}
fn ingest_config() -> IngestConfig {
    IngestConfig {
        start: 0,
        end: Some(3),
        confirmations: 1,
        reorg_depth: 144,
        poll_seconds: 1,
        verbosity: 2,
    }
}
fn database(name: &str) -> Database {
    Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:9".into())
            .parse()
            .unwrap(),
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: name.into(),
    })
    .unwrap()
}

#[test]
fn actual_mutating_cli_requires_explicit_shared_lock_and_rejects_another_owner() {
    let directory =
        std::env::temp_dir().join(format!("bitcoin-mutating-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let owner = WriterLock::acquire(&directory, "bitcoin_aml").unwrap();
    for (binary, args) in [
        (env!("CARGO_BIN_EXE_bitcoin_init_schema"), vec![]),
        (env!("CARGO_BIN_EXE_bitcoin_aml"), vec!["schema"]),
        (
            env!("CARGO_BIN_EXE_bitcoin_aml"),
            vec!["warehouse-backfill", "--max-blocks", "1"],
        ),
        (env!("CARGO_BIN_EXE_bitcoin_aml"), vec!["once"]),
        (
            env!("CARGO_BIN_EXE_bitcoin_aml"),
            vec![
                "repair",
                "--failure-id",
                "bitcoin:block:3",
                "--max-blocks",
                "1",
            ],
        ),
        (
            env!("CARGO_BIN_EXE_bitcoin_aml"),
            vec![
                "scan-gaps",
                "--from-block",
                "0",
                "--to-block",
                "3",
                "--max-blocks",
                "4",
            ],
        ),
    ] {
        for configured in [true, false] {
            let mut command = std::process::Command::new(binary);
            command
                .current_dir(&directory)
                .env_clear()
                .env("BITCOIN_CLICKHOUSE_PASSWORD", "private-fixture-secret")
                .env("BITCOIN_CLICKHOUSE_URL", "http://127.0.0.1:9")
                .env("BITCOIN_RPC_URL", "http://127.0.0.1:9")
                .env("BITCOIN_RPC_USER", "fixture")
                .env("BITCOIN_RPC_PASSWORD", "private-fixture-secret")
                .args(&args);
            if configured {
                command.env("BITCOIN_WRITER_LOCK_DIR", &directory);
            }
            let output = command.output().unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains(if configured {
                "ownership"
            } else {
                "BITCOIN_WRITER_LOCK_DIR"
            }));
            assert!(!error.contains("private-fixture-secret"));
        }
    }
    drop(owner);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn rpc_batch_correlates_reversed_results_and_rejects_bad_envelopes() {
    let mock = Mock::start(Settings::default()).await;
    let result = mock
        .rpc()
        .batch(vec![
            ("getblockhash", json!([0])),
            ("getblockhash", json!([1])),
        ])
        .await
        .unwrap();
    assert_eq!(result, vec![json!(hash(0)), json!(hash(1))]);
    for fault in 1..=3 {
        mock.state.settings.write().unwrap().batch_fault = fault;
        assert!(
            mock.rpc()
                .batch(vec![
                    ("getblockhash", json!([0])),
                    ("getblockhash", json!([1]))
                ])
                .await
                .is_err()
        );
    }
    assert!(mock.rpc().batch(vec![]).await.is_err());
    assert!(
        mock.rpc()
            .call("sendrawtransaction", json!([]))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rpc_transient_retry_is_bounded_and_private_errors_are_redacted() {
    for status in [429, 503] {
        let mock = Mock::start(Settings {
            fail_first: 2,
            http_status: status,
            ..Settings::default()
        })
        .await;
        let rpc = mock
            .rpc()
            .with_policy(RpcPolicy {
                attempts: 3,
                retry_base_ms: 5,
                ..RpcPolicy::default()
            })
            .unwrap();
        assert_eq!(rpc.height().await.unwrap(), 3);
        assert_eq!(mock.state.count.load(Ordering::SeqCst), 3);
    }
    let mock = Mock::start(Settings {
        fail_first: 99,
        http_status: 503,
        ..Settings::default()
    })
    .await;
    let rpc = mock
        .rpc()
        .with_policy(RpcPolicy {
            attempts: 2,
            retry_base_ms: 1,
            ..RpcPolicy::default()
        })
        .unwrap();
    let error = rpc.height().await.unwrap_err().to_string();
    assert!(!error.contains("private-fixture-secret") && !error.contains(&mock.url));
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 2);
    let mock = Mock::start(Settings {
        fail_first: 99,
        http_status: 401,
        ..Settings::default()
    })
    .await;
    let rpc = mock
        .rpc()
        .with_policy(RpcPolicy {
            attempts: 4,
            ..RpcPolicy::default()
        })
        .unwrap();
    assert!(rpc.height().await.is_err());
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rpc_warmup_retries_but_permanent_rpc_errors_do_not() {
    let mock = Mock::start(Settings {
        rpc_error: Some(-28),
        ..Settings::default()
    })
    .await;
    let rpc = mock
        .rpc()
        .with_policy(RpcPolicy {
            attempts: 3,
            retry_base_ms: 1,
            ..RpcPolicy::default()
        })
        .unwrap();
    assert!(rpc.height().await.is_err());
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 3);
    mock.state.settings.write().unwrap().rpc_error = Some(-5);
    assert!(rpc.height().await.is_err());
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn http_server_failure_cannot_be_bypassed_by_a_valid_result_body() {
    let mock = Mock::start(Settings {
        result_status: 500,
        ..Settings::default()
    })
    .await;
    let rpc = mock
        .rpc()
        .with_policy(RpcPolicy {
            attempts: 2,
            ..RpcPolicy::default()
        })
        .unwrap();
    assert!(rpc.height().await.is_err());
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn rate_gate_is_shared_across_clones_and_response_size_is_bounded() {
    let mock = Mock::start(Settings::default()).await;
    let rpc = mock
        .rpc()
        .with_policy(RpcPolicy {
            interval_ms: 40,
            ..RpcPolicy::default()
        })
        .unwrap();
    let started = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..3 {
        let rpc = rpc.clone();
        tasks.spawn(async move { rpc.height().await.unwrap() });
    }
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap(), 3);
    }
    assert!(started.elapsed() >= Duration::from_millis(75));
    mock.state.settings.write().unwrap().huge = true;
    assert!(
        mock.rpc()
            .with_policy(RpcPolicy {
                max_response_bytes: 1024,
                ..RpcPolicy::default()
            })
            .unwrap()
            .height()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn provider_without_batch_uses_explicit_bounded_single_request_mode() {
    let mock = Mock::start(Settings::default()).await;
    let rpc = mock
        .rpc()
        .with_policy(RpcPolicy {
            batch_enabled: false,
            ..RpcPolicy::default()
        })
        .unwrap();
    let result = rpc
        .batch(vec![
            ("getblockhash", json!([0])),
            ("getblockhash", json!([1])),
        ])
        .await
        .unwrap();
    assert_eq!(result.len(), 2);
    assert!(mock.requests().iter().all(|r| r.is_object()));
}

#[tokio::test]
async fn parallel_fetch_is_bounded_sorted_and_rejects_non_contiguous_windows() {
    let mock = Mock::start(Settings {
        slow_first: true,
        ..Settings::default()
    })
    .await;
    let blocks = fetch::blocks(&mock.rpc(), 0, 3, 3, 2, 2).await.unwrap();
    assert_eq!(
        blocks
            .iter()
            .map(|b| b["height"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(mock.state.max_inflight.load(Ordering::SeqCst), 2);
    assert_eq!(mock.state.completion.lock().unwrap()[0], 1);
    mock.state.settings.write().unwrap().broken_window = true;
    assert!(fetch::blocks(&mock.rpc(), 0, 3, 3, 2, 2).await.is_err());
}

#[tokio::test]
async fn competing_mutating_entrypoints_fail_before_database_or_rpc_access() {
    let mock = Mock::start(Settings::default()).await;
    let db = database("bitcoin_writer_refusal");
    let _owner = WriterLock::for_database("bitcoin_writer_refusal").unwrap();
    assert!(
        ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
            .await
            .unwrap_err()
            .to_string()
            .contains("ownership")
    );
    assert!(
        warehouse::backfill(&db, 1)
            .await
            .unwrap_err()
            .to_string()
            .contains("ownership")
    );
    assert!(mock.requests().is_empty());
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn ordered_pipeline_batches_local_and_raw_prevouts_without_touching_legacy() {
    let db = database("bitcoin_ing02_ordered");
    db.initialize().await.unwrap();
    let mock = Mock::start(Settings {
        slow_first: true,
        ..Settings::default()
    })
    .await;
    let tuning = PipelineConfig {
        block_batch: 4,
        block_concurrency: 2,
        prevout_batch: 2,
        ..PipelineConfig::default()
    };
    ingestion::run_pool(
        db.clone(),
        RpcPool::single(mock.rpc()),
        ingest_config(),
        tuning,
        true,
    )
    .await
    .unwrap();
    assert_eq!(db.state().await.unwrap().tip_height, 3);
    let rows: Vec<Value> = db
        .rows(
            "SELECT block_number FROM aml_ingested_blocks_canonical ORDER BY block_number",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r["block_number"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    let calls = mock.calls("getrawtransaction");
    assert_eq!(calls.len(), 2);
    assert!(mock.requests().iter().any(|r| {
        r.as_array()
            .is_some_and(|a| a.len() == 2 && a[0]["method"] == "getrawtransaction")
    }));
    let rows:Vec<Value>=db.rows("SELECT toString(sum(value_sats)) AS amount FROM aml_bitcoin_inputs_canonical WHERE block_number=3",&[]).await.unwrap();
    assert_eq!(rows[0]["amount"], "900000000");
    let rows:Vec<Value>=db.rows("SELECT count() AS n FROM system.tables WHERE database=currentDatabase() AND startsWith(name,'bitcoin_')",&[]).await.unwrap();
    assert_eq!(rows[0]["n"], 0);
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn missing_prevout_retains_exact_prefix_and_restart_resumes_after_repair() {
    let db = database("bitcoin_ing02_recovery");
    db.initialize().await.unwrap();
    let mock = Mock::start(Settings {
        missing_raw: true,
        ..Settings::default()
    })
    .await;
    assert!(
        ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
            .await
            .is_err()
    );
    let before = db.state().await.unwrap();
    assert_eq!(before.tip_height, 2);
    let rows: Vec<Value> = db
        .rows(
            "SELECT count() AS n FROM aml_transactions_canonical WHERE block_number=3",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 0);
    mock.state.settings.write().unwrap().missing_raw = false;
    ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
        .await
        .unwrap();
    assert_eq!(db.state().await.unwrap().tip_height, 3);
    let rows: Vec<Value> = db
        .rows(
            "SELECT count() AS n FROM aml_ingested_blocks_canonical",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 4);
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn fallback_is_validated_and_cannot_rollback_on_a_shorter_or_divergent_chain() {
    let db = database("bitcoin_ing02_fallback");
    db.initialize().await.unwrap();
    let healthy = Mock::start(Settings::default()).await;
    ingestion::run(
        db.clone(),
        healthy.rpc(),
        IngestConfig {
            end: Some(1),
            ..ingest_config()
        },
        true,
    )
    .await
    .unwrap();
    let unavailable = Mock::start(Settings {
        fail_first: 999,
        http_status: 503,
        ..Settings::default()
    })
    .await;
    let mut short = Settings::default();
    short.blocks.truncate(1);
    let shorter = Mock::start(short).await;
    let mut divergent = Settings::default();
    divergent.blocks[1]["hash"] = json!("d".repeat(64));
    let fork = Mock::start(divergent).await;
    let wrong = Mock::start(Settings {
        chain: "signet",
        ..Settings::default()
    })
    .await;
    let before = db.state().await.unwrap();
    let pool = RpcPool::new(vec![
        unavailable.rpc(),
        shorter.rpc(),
        fork.rpc(),
        wrong.rpc(),
    ])
    .unwrap();
    assert!(
        ingestion::run_pool(
            db.clone(),
            pool,
            ingest_config(),
            PipelineConfig::default(),
            true
        )
        .await
        .is_err()
    );
    assert_eq!(db.state().await.unwrap().revision, before.revision);
    assert!(
        shorter.calls("getblock").is_empty()
            && fork.calls("getblock").is_empty()
            && wrong.calls("getblock").is_empty()
    );
    let pool = RpcPool::new(vec![unavailable.rpc(), healthy.rpc()]).unwrap();
    ingestion::run_pool(
        db.clone(),
        pool,
        ingest_config(),
        PipelineConfig::default(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(db.state().await.unwrap().tip_height, 3);
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn prevout_work_limit_and_two_writers_are_enforced_before_publication() {
    let db = database("bitcoin_ing02_limits");
    db.initialize().await.unwrap();
    let mock = Mock::start(Settings::default()).await;
    let tuning = PipelineConfig {
        max_prevouts: 1,
        ..PipelineConfig::default()
    };
    assert!(
        resolver::resolve(&mock.rpc(), &db, &block(3), &tuning)
            .await
            .is_err()
    );
    assert!(mock.calls("getrawtransaction").is_empty());
    assert_eq!(db.state().await.unwrap().revision, 0);
    mock.state.settings.write().unwrap().raw_script_bytes = 1024;
    let budget = PipelineConfig {
        max_resolved_bytes: 1024,
        ..PipelineConfig::default()
    };
    assert!(
        resolver::resolve(&mock.rpc(), &db, &block(3), &budget)
            .await
            .unwrap_err()
            .to_string()
            .contains("byte budget")
    );
    mock.state.settings.write().unwrap().raw_script_bytes = 0;
    let writer_db = db.clone();
    let writer_rpc = mock.rpc();
    let worker = tokio::spawn(async move {
        ingestion::run(
            writer_db,
            writer_rpc,
            IngestConfig {
                end: None,
                ..ingest_config()
            },
            false,
        )
        .await
    });
    for _ in 0..200 {
        if db.state().await.unwrap().initialized == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
            .await
            .unwrap_err()
            .to_string()
            .contains("ownership")
    );
    worker.abort();
    let _ = worker.await;
    ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
        .await
        .unwrap();
    assert_eq!(db.state().await.unwrap().tip_height, 3);
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn fallback_continues_after_partial_window_without_duplicate_publication() {
    let db = database("bitcoin_ing02_partial_fallback");
    db.initialize().await.unwrap();
    let primary = Mock::start(Settings {
        missing_raw: true,
        ..Settings::default()
    })
    .await;
    let fallback = Mock::start(Settings::default()).await;
    let pool = RpcPool::new(vec![primary.rpc(), fallback.rpc()]).unwrap();
    ingestion::run_pool(
        db.clone(),
        pool,
        ingest_config(),
        PipelineConfig {
            block_batch: 4,
            ..PipelineConfig::default()
        },
        true,
    )
    .await
    .unwrap();
    assert_eq!(db.state().await.unwrap().tip_height, 3);
    let rows: Vec<Value> = db
        .rows(
            "SELECT count() AS n FROM aml_ingested_blocks_canonical",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 4);
    assert_eq!(fallback.calls("getblock").len(), 1);
    assert_eq!(fallback.calls("getblock")[0]["params"][0], hash(3));
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn follow_survives_rpc_outage_and_recovers_without_restart() {
    let db = database("bitcoin_ing02_follow");
    db.initialize().await.unwrap();
    let mock = Mock::start(Settings {
        fail_first: 999,
        http_status: 503,
        ..Settings::default()
    })
    .await;
    let worker_db = db.clone();
    let rpc = mock.rpc();
    let worker = tokio::spawn(async move {
        ingestion::run(
            worker_db,
            rpc,
            IngestConfig {
                end: None,
                ..ingest_config()
            },
            false,
        )
        .await
    });
    for _ in 0..200 {
        if mock.state.count.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!worker.is_finished());
    assert_eq!(db.state().await.unwrap().revision, 0);
    mock.state.settings.write().unwrap().fail_first = 0;
    for _ in 0..500 {
        if db.state().await.unwrap().tip_height == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(db.state().await.unwrap().tip_height, 3);
    assert!(!worker.is_finished());
    worker.abort();
    let _ = worker.await;
}

#[test]
fn failure_tool_help_and_strict_arguments_work_without_database_or_rpc_settings() {
    let directory = std::env::temp_dir();
    for mode in ["failures", "scan-gaps", "repair"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_bitcoin_aml"))
            .env_clear()
            .current_dir(&directory)
            .args([mode, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
    }
    for (mode, args) in [
        (
            "repair",
            vec!["--failure-id", "bitcoin:block:03", "--max-blocks", "1"],
        ),
        (
            "repair",
            vec![
                "--failure-id",
                "bitcoin:block:3",
                "--failure-id",
                "bitcoin:block:3",
                "--max-blocks",
                "2",
            ],
        ),
        ("repair", vec!["--failure-id", "bitcoin:block:3"]),
        (
            "scan-gaps",
            vec!["--from-block", "4", "--to-block", "3", "--max-blocks", "2"],
        ),
        (
            "scan-gaps",
            vec!["--from-block", "0", "--to-block", "4", "--max-blocks", "4"],
        ),
        ("failures", vec!["--limit", "201"]),
        ("failures", vec!["--status", "retry' OR 1=1"]),
    ] {
        assert!(
            repair::Arguments::parse(
                mode,
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>()
            )
            .is_err()
        );
    }
    assert!(repair::Arguments::parse("failures", &[]).is_ok());
    assert!(
        failures::Policy {
            max_attempts: 0,
            retry_seconds: 1
        }
        .validate()
        .is_err()
    );
}

#[test]
fn dedicated_schema_binary_and_main_alias_export_the_same_active_migration_without_settings() {
    let expected = bitcoin_aml::db::schema_sql().unwrap();
    for (binary, args) in [
        (
            env!("CARGO_BIN_EXE_bitcoin_init_schema"),
            vec!["--print-sql"],
        ),
        (
            env!("CARGO_BIN_EXE_bitcoin_aml"),
            vec!["schema", "--print-sql"],
        ),
    ] {
        let output = std::process::Command::new(binary)
            .env_clear()
            .current_dir(std::env::temp_dir())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

async fn selected_repair(
    db: &Database,
    mock: &Mock,
    heights: &[u64],
    policy: failures::Policy,
) -> Value {
    repair::run(
        db.clone(),
        RpcPool::single(mock.rpc()),
        ingest_config(),
        PipelineConfig::default(),
        repair::Options {
            ids: heights.iter().map(|h| failures::id(*h)).collect(),
            max_blocks: heights.len() as u64,
            policy,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn durable_dead_letter_blocks_restart_and_selected_repair_preserves_attempts() {
    let db = database("bitcoin_ing03_dead");
    db.initialize().await.unwrap();
    let mock = Mock::start(Settings {
        missing_raw: true,
        ..Settings::default()
    })
    .await;
    let policy = failures::Policy {
        max_attempts: 2,
        retry_seconds: 3600,
    };
    for attempt in 1..=2 {
        assert!(
            ingestion::run_with_policy(
                db.clone(),
                RpcPool::single(mock.rpc()),
                ingest_config(),
                PipelineConfig::default(),
                policy.clone(),
                true
            )
            .await
            .is_err()
        );
        let row = failures::current(&db, 3).await.unwrap().unwrap();
        assert_eq!(row.attempts, attempt);
        assert_eq!(
            row.status,
            if attempt == 1 { "retry" } else { "dead_letter" }
        );
        assert!(
            !serde_json::to_string(&row)
                .unwrap()
                .contains("private-fixture-secret")
        );
        assert_eq!(db.state().await.unwrap().tip_height, 2);
    }
    let row = failures::current(&db, 3).await.unwrap().unwrap();
    assert_eq!(
        failures::gate(&db, 3, &policy, false).await.unwrap(),
        Some("dead_letter")
    );
    let request_count = mock.requests().len();
    assert!(
        ingestion::run_with_policy(
            db.clone(),
            RpcPool::single(mock.rpc()),
            ingest_config(),
            PipelineConfig::default(),
            policy.clone(),
            true
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("dead_letter")
    );
    assert_eq!(mock.requests().len(), request_count);
    let failed = selected_repair(&db, &mock, &[3], policy.clone()).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(
        failures::current(&db, 3).await.unwrap().unwrap().attempts,
        3
    );
    mock.state.settings.write().unwrap().missing_raw = false;
    let report = selected_repair(&db, &mock, &[3], policy).await;
    assert_eq!(report["status"], "complete");
    assert_eq!(report["checkpoint_changed"], true);
    let repaired = failures::current(&db, 3).await.unwrap().unwrap();
    assert_eq!(repaired.status, "resolved");
    assert_eq!(repaired.attempts, 4);
    assert!(repaired.revision > row.revision);
    assert_eq!(repaired.created_at_unix_ms, row.created_at_unix_ms);
    let rows: Vec<Value> = db
        .rows(
            "SELECT count() AS n FROM aml_ingested_blocks_canonical",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 4);
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn fallback_success_resolves_persisted_failure_with_one_successful_attempt() {
    let db = database("bitcoin_ing03_fallback");
    db.initialize().await.unwrap();
    let primary = Mock::start(Settings {
        missing_raw: true,
        ..Settings::default()
    })
    .await;
    let fallback = Mock::start(Settings::default()).await;
    ingestion::run_pool(
        db.clone(),
        RpcPool::new(vec![primary.rpc(), fallback.rpc()]).unwrap(),
        ingest_config(),
        PipelineConfig::default(),
        true,
    )
    .await
    .unwrap();
    let row = failures::current(&db, 3).await.unwrap().unwrap();
    assert_eq!(row.status, "resolved");
    assert_eq!(row.attempts, 2);
    assert_eq!(row.error_code, "block_evidence_unavailable");
    assert_eq!(row.block_hash, Some(hash(3)));
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn interrupted_repair_clock_rollback_and_uncertain_publication_reconcile_durably() {
    let db = database("bitcoin_ing03_interrupted");
    db.initialize().await.unwrap();
    let policy = failures::Policy {
        max_attempts: 2,
        retry_seconds: 3600,
    };
    let mut row = failures::record(&db, 0, None, failures::Stage::Source, &policy, true)
        .await
        .unwrap();
    assert_eq!(
        failures::gate(&db, 0, &policy, false).await.unwrap(),
        Some("retry_not_due")
    );
    assert_eq!(failures::gate(&db, 0, &policy, true).await.unwrap(), None);
    row.revision = u64::MAX - 1000;
    db.execute(&format!(
        "INSERT INTO aml_ingestion_failures FORMAT JSONEachRow\n{}",
        serde_json::to_string(&row).unwrap()
    ))
    .await
    .unwrap();
    let reserved = failures::begin_repair(&db, 0).await.unwrap();
    assert_eq!(reserved.attempts, 2);
    assert!(reserved.revision > row.revision);
    failures::recover_interrupted(&db, &policy).await.unwrap();
    let interrupted = failures::current(&db, 0).await.unwrap().unwrap();
    assert_eq!(interrupted.attempts, 2);
    assert_eq!(interrupted.status, "dead_letter");
    assert_eq!(interrupted.error_code, "interrupted_repair");
    assert!(interrupted.revision > reserved.revision);
    let mock = Mock::start(Settings::default()).await;
    assert_eq!(
        selected_repair(&db, &mock, &[0], policy.clone()).await["status"],
        "complete"
    );
    assert_eq!(db.state().await.unwrap().tip_height, 0); // only the selected block
    ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
        .await
        .unwrap();
    failures::record(
        &db,
        2,
        Some(&hash(2)),
        failures::Stage::Commit,
        &policy,
        true,
    )
    .await
    .unwrap();
    let before = db.state().await.unwrap();
    ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
        .await
        .unwrap();
    assert_eq!(db.state().await.unwrap(), before);
    let settled = failures::current(&db, 2).await.unwrap().unwrap();
    assert_eq!(settled.status, "resolved");
    assert_eq!(settled.attempts, 1);
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn batch_cannot_skip_dead_letter_and_repair_cannot_skip_unselected_blocks() {
    let db = database("bitcoin_ing03_order");
    db.initialize().await.unwrap();
    let policy = failures::Policy {
        max_attempts: 1,
        retry_seconds: 1,
    };
    failures::record(&db, 1, None, failures::Stage::Fetch, &policy, true)
        .await
        .unwrap();
    let mock = Mock::start(Settings::default()).await;
    assert!(
        ingestion::run_with_policy(
            db.clone(),
            RpcPool::single(mock.rpc()),
            ingest_config(),
            PipelineConfig {
                block_batch: 4,
                ..PipelineConfig::default()
            },
            policy.clone(),
            true
        )
        .await
        .is_err()
    );
    assert_eq!(db.state().await.unwrap().tip_height, 0);
    assert_eq!(
        failures::current(&db, 1).await.unwrap().unwrap().attempts,
        1
    );
    failures::record(&db, 3, None, failures::Stage::Fetch, &policy, true)
        .await
        .unwrap();
    let before = db.state().await.unwrap();
    let report = selected_repair(&db, &mock, &[3], policy.clone()).await;
    assert_eq!(report["status"], "blocked");
    assert_eq!(db.state().await.unwrap(), before);
    let report = selected_repair(&db, &mock, &[1], policy).await;
    assert_eq!(report["status"], "complete");
    assert_eq!(db.state().await.unwrap().tip_height, 1);
    assert_eq!(
        failures::current(&db, 3).await.unwrap().unwrap().status,
        "dead_letter"
    );
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn gap_scan_records_missing_blocks_and_transactions_without_rewinding_history() {
    let db = database("bitcoin_ing03_gap");
    db.initialize().await.unwrap();
    let mock = Mock::start(Settings::default()).await;
    ingestion::run(db.clone(), mock.rpc(), ingest_config(), true)
        .await
        .unwrap();
    assert_eq!(repair::scan(&db, 0, 3, 4).await.unwrap()["gaps_found"], 0);
    assert!(repair::scan(&db, 0, 4, 5).await.is_err());
    assert!(repair::scan(&db, 0, 3, 3).await.is_err());
    // Corrupt disposable fixture facts only; production scan/repair never DELETEs.
    db.execute(
        "ALTER TABLE aml_ingested_blocks DELETE WHERE block_number=1 SETTINGS mutations_sync=2",
    )
    .await
    .unwrap();
    db.execute(
        "ALTER TABLE aml_transactions DELETE WHERE block_number=3 SETTINGS mutations_sync=2",
    )
    .await
    .unwrap();
    let before = db.state().await.unwrap();
    let report = repair::scan(&db, 0, 3, 4).await.unwrap();
    assert_eq!(report["gaps_found"], 2);
    assert_eq!(
        failures::current(&db, 1).await.unwrap().unwrap().status,
        "gap"
    );
    let result = selected_repair(&db, &mock, &[1], failures::Policy::default()).await;
    assert_eq!(result["status"], "blocked");
    assert_eq!(
        result["results"][0]["reason"],
        "historical_gap_requires_replay"
    );
    assert_eq!(db.state().await.unwrap(), before);
    assert_eq!(
        failures::current(&db, 1).await.unwrap().unwrap().attempts,
        0
    );
}

#[tokio::test]
#[ignore = "requires disposable ClickHouse"]
async fn failure_api_is_protected_filtered_paginated_and_network_scoped() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let db = database("bitcoin_ing03_api");
    db.initialize().await.unwrap();
    let policy = failures::Policy::default();
    for height in [3, 4, 5] {
        failures::record(&db, height, None, failures::Stage::Fetch, &policy, true)
            .await
            .unwrap();
    }
    let mut foreign = failures::current(&db, 3).await.unwrap().unwrap();
    foreign.network_id = aml_warehouse::Network::Ethereum.id().into();
    foreign.failure_id = "foreign-fixture".into();
    db.execute(&format!(
        "INSERT INTO aml_ingestion_failures FORMAT JSONEachRow\n{}",
        serde_json::to_string(&foreign).unwrap()
    ))
    .await
    .unwrap();
    let app = bitcoin_aml::api::router(bitcoin_aml::api::Api::new(db, "k".repeat(32)).unwrap());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/bitcoin/ingestion/failures")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    for (uri, status, expected, next) in [
        (
            "/api/bitcoin/ingestion/failures?status=retry&limit=2",
            200,
            2,
            Some(4),
        ),
        (
            "/api/bitcoin/ingestion/failures?status=retry&after_block=4&limit=2",
            200,
            1,
            None,
        ),
        (
            "/api/bitcoin/ingestion/failures?from_block=4&to_block=4",
            200,
            1,
            None,
        ),
        (
            "/api/bitcoin/ingestion/failures?status=dead_letter",
            200,
            0,
            None,
        ),
        (
            "/api/bitcoin/ingestion/failures?status=invalid",
            400,
            0,
            None,
        ),
        ("/api/bitcoin/ingestion/failures?limit=201", 400, 0, None),
        (
            "/api/bitcoin/ingestion/failures?from_block=5&to_block=4",
            400,
            0,
            None,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("X-AML-Service-Key", "k".repeat(32))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        if status == 200 {
            let value: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                    .unwrap();
            assert_eq!(value["failures"].as_array().unwrap().len(), expected);
            assert_eq!(value["next_after_block"].as_u64(), next);
            assert!(
                value["failures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["network_id"] == bitcoin_aml::NETWORK_ID)
            );
        }
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("X-AML-Service-Key", "k".repeat(32))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(value["ingestion_failures"]["counts"]["retry"], 3);
    assert_eq!(
        value["ingestion_failures"]["next_failure"]["block_number"],
        3
    );
}
