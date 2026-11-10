use super::*;
use crate::{Store, user_owner_hash};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn receipt(network: Network) -> Log {
    let (topic, token, amount, remote, tx, timestamp_ms) = if network == Network::Ethereum {
        (
            "89d8051e597ab4178a863a5190407b98abfeff406aa8db90c59af76612e58f01",
            "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            100_000_000,
            56,
            "a",
            1_700_000_000_000,
        )
    } else {
        (
            "79fa08de5149d912dce8e5e8da7a7c17ccdf23dd5d3bfe196802e6eb86347c7c",
            "8ac76a51cc950d9822d68b83fe1ad97b32cd580d",
            99_000_000_000_000_000_000u128,
            1,
            "b",
            1_700_000_030_000,
        )
    };
    let operation = "1".repeat(64);
    let event = if network == Network::Ethereum {
        operation.clone()
    } else {
        "2".repeat(64)
    };
    let tail = if network == Network::Ethereum {
        format!("{:064x}{:064x}", 1, 10_000)
    } else {
        operation
    };
    Log {
        tx_hash: format!("0x{}", tx.repeat(64)),
        block_hash: format!("0x{}", "9".repeat(64)),
        block_number: 10,
        timestamp_ms,
        log_index: 2,
        address: bridge::pool(network).unwrap().into(),
        topics: vec![format!("0x{topic}")],
        data: format!(
            "{event}{:0>64}{:0>64}{token:0>64}{amount:064x}{remote:064x}{tail}",
            "3".repeat(40),
            "4".repeat(40)
        ),
    }
}

#[test]
fn amount_time_chain_token_and_operation_must_agree() {
    let mut send = receipt(Network::Ethereum);
    let mut relay = receipt(Network::Bsc);
    let evidence = Value::Null;
    let matches = |send: &Log, relay: &Log| {
        matched_pair(
            (
                Network::Ethereum,
                send,
                &send.decode(Network::Ethereum)?,
                &evidence,
            ),
            (Network::Bsc, relay, &relay.decode(Network::Bsc)?, &evidence),
        )
    };
    let trace = matches(&send, &relay).unwrap();
    assert_eq!(trace["asset"], "USDC");
    assert_eq!(trace["send"]["amount_raw"], "100000000");
    assert_eq!(trace["relay"]["amount_raw"], "99000000000000000000");
    assert_eq!(trace["settlement_verified"], false);
    let original = relay.clone();
    relay.timestamp_ms = send.timestamp_ms + WINDOW_MS + 1;
    assert!(matches(&send, &relay).is_none());
    relay.timestamp_ms = send.timestamp_ms - 1;
    assert!(matches(&send, &relay).is_none());
    for (range, value) in [
        (384..385, "f"),             // Relay's source operation ID
        (256..320, &"f".repeat(64)), // uint256 amount outside tolerance
        (383..384, "2"),             // source chain
        (128 + 24..128 + 25, "f"),   // receiver
        (192 + 24..192 + 25, "f"),   // token
    ] {
        relay = original.clone();
        relay.data.replace_range(range, value);
        assert!(matches(&send, &relay).is_none());
    }
    relay = original;
    send.data.replace_range(64..65, "1"); // nonzero address padding
    assert!(matches(&send, &relay).is_none());
}

async fn serve(router: Router) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Ok((format!("http://{address}"), task))
}

#[tokio::test]
#[ignore = "requires a disposable BRIDGE_NEO4J_URL and BRIDGE_NEO4J_PASSWORD"]
async fn tbtc_mint_http_persists_only_a_complete_issuance() -> anyhow::Result<()> {
    use aml_chain_api::bridge::tbtc;
    use axum::extract::Query;
    use std::sync::atomic::AtomicU8;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../shared/chain_api/tests/fixtures/tbtc-mint-mainnet.json"
    ))?;
    let btc = fixture["btc"].clone();
    let reveal = fixture["eth"].clone();
    let finalization = fixture["mint"][2].clone();
    let neighbors = fixture["mint"].as_array().unwrap()[..2].to_vec();
    let bitcoin = Network::Bitcoin;
    let ethereum = Network::Ethereum;
    let snapshot = |network: Network, start: &Value, tip: &Value| {
        json!({
            "network_id":network.id(),"contract":"canonical_v1","snapshot_id":"e".repeat(64),
            "initialized":true,"start_block":start["block_number"],"as_of_block":tip["block_number"],
            "block_hash":tip["block_hash"]
        })
    };
    let btc_snapshot = snapshot(bitcoin, &btc, &btc);
    let eth_snapshot = snapshot(ethereum, &reveal, &finalization);
    let btc_status = btc_snapshot.clone();
    let eth_status = eth_snapshot.clone();
    let mint_snapshot = eth_snapshot.clone();
    let txid = btc["tx_hash"].clone();
    let block = btc["block_number"].clone();
    let funding_hash = txid.clone();
    let (btc_url, btc_task) = serve(Router::new()
        .route(tbtc::FUNDING_ROUTE, get(move |Path(hash): Path<String>, Query(query): Query<tbtc::FundingQuery>| {
            let outputs = if btc["tx_hash"] == hash && btc["block_number"] == query.block_number
                && query.output_index.is_none_or(|i| i == 0) { vec![btc.clone()] } else { vec![] };
            let response = json!({"network_id":bitcoin.id(),"source_snapshot":btc_snapshot,
                "outputs":outputs,"truncated":false});
            async { Json(response) }
        }))
        .route(bridge::SNAPSHOT_ROUTE, get(move || { let status = btc_status.clone(); async { Json(status) } }))).await?;
    let mode = Arc::new(AtomicU8::new(0));
    let observed_mode = mode.clone();
    let (eth_url, eth_task) = serve(Router::new()
        .route(tbtc::REVEALS_ROUTE, get(move |Path(hash): Path<String>, Query(window): Query<tbtc::RevealWindow>| {
            let logs = if funding_hash == hash && window.output_index.is_none_or(|i| i == 0)
                && (window.from_ms..=window.to_ms).contains(&reveal["timestamp_ms"].as_u64().unwrap())
                { vec![reveal.clone()] } else { vec![] };
            let response = json!({"network_id":ethereum.id(),"source_snapshot":eth_snapshot,
                "logs":logs,"truncated":false});
            async { Json(response) }
        }))
        .route(tbtc::MINTS_ROUTE, get(move |Path(key): Path<String>, Query(window): Query<tbtc::MintWindow>| {
            let mode = observed_mode.load(Ordering::SeqCst);
            let mut snapshot = mint_snapshot.clone();
            if mode == 1 { snapshot["snapshot_id"] = json!("f".repeat(64)); }
            let hit = finalization["topics"][2] == key
                && (window.from_ms..=window.to_ms).contains(&finalization["timestamp_ms"].as_u64().unwrap());
            let logs = if hit { vec![finalization.clone()] } else { vec![] };
            let preceding = if hit && mode != 0 { neighbors.as_slice() } else { &[] };
            let response = json!({"finalizations":{"network_id":ethereum.id(),"source_snapshot":snapshot,
                "logs":logs,"truncated":false},"preceding_logs":preceding});
            async { Json(response) }
        }))
        .route(bridge::SNAPSHOT_ROUTE, get(move || { let status = eth_status.clone(); async { Json(status) } }))).await?;
    let http = reqwest::Client::new();
    let store = Store::new(
        http.clone(),
        &std::env::var("BRIDGE_NEO4J_URL")?,
        std::env::var("BRIDGE_NEO4J_PASSWORD")?,
    );
    store.initialize().await?;
    let (api, task) = serve(routes().with_state(App {
        store: store.clone(),
        http: http.clone(),
        key: "bridge-fixture".into(),
        ttl_ms: 60_000,
        requests: Arc::new(tokio::sync::Semaphore::new(2)),
        upstreams: HashMap::from([
            (bitcoin.name().into(), btc_url),
            (ethereum.name().into(), eth_url),
        ]),
    }))
    .await?;
    let id = nanoid::nanoid!(24);
    let owner = user_owner_hash("bridge-owner");
    store.create(json!({
        "metadata":{"id":id,"state":"temporary","network":bitcoin.name(),"network_id":bitcoin.id(),"expires_at_unix_ms":now()+60_000},
        "owner":owner,"payload":json!({"graph":{"edges":[{"tx_hash":txid,"block_number":block}]}}).to_string(),
        "nodes":[],"edges":[]
    })).await?;
    let post = || {
        http.post(format!("{api}/api/investigations/{id}/bridges"))
            .header("x-aml-user", "bridge-owner")
            .header("x-aml-service-key", "bridge-fixture")
            .json(&json!({"tx_hash":txid,"output_index":0,"verify_mint":true}))
            .send()
    };
    let incomplete: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(incomplete["status"], "inconsistent_mint");
    mode.store(1, Ordering::SeqCst);
    let stale: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(stale["status"], "source_changed");
    assert!(
        store
            .bridge_traces(&id, &owner, now())
            .await?
            .unwrap()
            .is_empty()
    );
    mode.store(2, Ordering::SeqCst);
    let matched: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(matched["status"], "matched");
    let trace = &matched["trace"];
    assert_eq!(trace["claim"], "optimistic_mint");
    assert_eq!(trace["mint_verified"], true);
    assert_eq!(trace["relay"]["amount_raw"], "10500000000000000");
    assert_eq!(trace["relay"]["token_transfer"]["log_index"], 119);
    let retry: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(retry, matched);
    assert_eq!(
        store.bridge_traces(&id, &owner, now()).await?.unwrap(),
        vec![trace.clone()]
    );
    task.abort();
    btc_task.abort();
    eth_task.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires a disposable BRIDGE_NEO4J_URL and BRIDGE_NEO4J_PASSWORD"]
async fn tbtc_http_persists_only_an_unchanged_canonical_pair() -> anyhow::Result<()> {
    use aml_chain_api::bridge::tbtc;
    use axum::extract::Query;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../shared/chain_api/tests/fixtures/tbtc-sweep-mainnet.json"
    ))?;
    let bitcoin = Network::Bitcoin;
    let ethereum = Network::Ethereum;
    let snapshot = |network: Network, start: &Value, tip: &Value| {
        json!({
            "network_id":network.id(),"contract":"canonical_v1","snapshot_id":"e".repeat(64),
            "initialized":true,"start_block":start["block_number"],"as_of_block":tip["block_number"],
            "block_hash":tip["block_hash"]
        })
    };
    let btc = fixture["btc"].clone();
    let eth = fixture["eth"].clone();
    let spend = fixture["spend"].clone();
    let proof = fixture["proof"].clone();
    let btc_snapshot = snapshot(bitcoin, &btc, &spend);
    let eth_snapshot = snapshot(ethereum, &eth, &proof);
    let txid = btc["tx_hash"].clone();
    let block = btc["block_number"].clone();
    let spend_block = block.clone();
    let spend_snapshot = btc_snapshot.clone();
    let proof_snapshot = eth_snapshot.clone();
    let sweep_tx = spend["tx_hash"].clone();
    let btc_status = btc_snapshot.clone();
    let eth_status = eth_snapshot.clone();
    let changed = Arc::new(AtomicBool::new(true));
    let source_changed = changed.clone();
    let (btc_url, btc_task) = serve(
        Router::new()
            .route(
                tbtc::FUNDING_ROUTE,
                get(
                    move |Path(hash): Path<String>, Query(query): Query<tbtc::FundingQuery>| {
                        let outputs = if btc["tx_hash"] == hash
                            && btc["block_number"] == query.block_number
                            && query.output_index.is_none_or(|i| btc["output_index"] == i)
                        {
                            vec![btc.clone()]
                        } else {
                            vec![]
                        };
                        let observation = Json(
                            json!({"network_id":bitcoin.id(),"source_snapshot":btc_snapshot,
                "outputs":outputs,"truncated":false}),
                        );
                        async { observation }
                    },
                ),
            )
            .route(
                tbtc::SPENDS_ROUTE,
                get(
                    move |Path(hash): Path<String>, Query(query): Query<tbtc::SpendQuery>| {
                        let inputs = if spend["prev_tx_hash"] == hash
                            && spend_block == query.block_number
                            && spend["prev_output_index"] == query.output_index
                        {
                            vec![spend.clone()]
                        } else {
                            vec![]
                        };
                        let observation = Json(json!({"network_id":bitcoin.id(),
                        "source_snapshot":spend_snapshot,"inputs":inputs}));
                        async { observation }
                    },
                ),
            )
            .route(
                bridge::SNAPSHOT_ROUTE,
                get(move || {
                    let mut status = btc_status.clone();
                    if source_changed.load(Ordering::SeqCst) {
                        status["snapshot_id"] = json!("f".repeat(64));
                    }
                    async { Json(status) }
                }),
            ),
    )
    .await?;
    let funding_hash = txid.clone();
    let proof_available = Arc::new(AtomicBool::new(false));
    let has_proof = proof_available.clone();
    let (eth_url, eth_task) = serve(
        Router::new()
            .route(
                tbtc::REVEALS_ROUTE,
                get(
                    move |Path(hash): Path<String>, Query(window): Query<tbtc::RevealWindow>| {
                        let logs = if funding_hash == hash
                            && (window.from_ms..=window.to_ms)
                                .contains(&eth["timestamp_ms"].as_u64().unwrap())
                            && window.output_index.is_none_or(|i| i == 0)
                        {
                            vec![eth.clone()]
                        } else {
                            vec![]
                        };
                        let observation = Json(
                            json!({"network_id":ethereum.id(),"source_snapshot":eth_snapshot,
                "logs":logs,"truncated":false}),
                        );
                        async { observation }
                    },
                ),
            )
            .route(
                tbtc::SWEEPS_ROUTE,
                get(
                    move |Path(hash): Path<String>, Query(window): Query<tbtc::SweepWindow>| {
                        let logs = if has_proof.load(Ordering::SeqCst)
                            && sweep_tx == hash
                            && (window.from_ms..=window.to_ms)
                                .contains(&proof["timestamp_ms"].as_u64().unwrap())
                        {
                            vec![proof.clone()]
                        } else {
                            vec![]
                        };
                        let observation = Json(json!({"network_id":ethereum.id(),
                        "source_snapshot":proof_snapshot,"logs":logs,"truncated":false}));
                        async { observation }
                    },
                ),
            )
            .route(
                bridge::SNAPSHOT_ROUTE,
                get(move || {
                    let status = eth_status.clone();
                    async { Json(status) }
                }),
            ),
    )
    .await?;
    let http = reqwest::Client::new();
    let store = Store::new(
        http.clone(),
        &std::env::var("BRIDGE_NEO4J_URL")?,
        std::env::var("BRIDGE_NEO4J_PASSWORD")?,
    );
    store.initialize().await?;
    let (api, task) = serve(routes().with_state(App {
        store: store.clone(),
        http: http.clone(),
        key: "bridge-fixture".into(),
        ttl_ms: 60_000,
        requests: Arc::new(tokio::sync::Semaphore::new(2)),
        upstreams: HashMap::from([
            (bitcoin.name().into(), btc_url),
            (ethereum.name().into(), eth_url),
        ]),
    }))
    .await?;
    let id = nanoid::nanoid!(24);
    let owner = user_owner_hash("bridge-owner");
    store.create(json!({
        "metadata":{"id":id,"state":"temporary","network":bitcoin.name(),"network_id":bitcoin.id(),"expires_at_unix_ms":now()+60_000},
        "owner":owner,"payload":json!({"graph":{"edges":[{"tx_hash":txid,"block_number":block}]}}).to_string(),
        "nodes":[],"edges":[]
    })).await?;
    let url = format!("{api}/api/investigations/{id}/bridges");
    let request = |body: Value| {
        http.post(&url)
            .header("x-aml-user", "bridge-owner")
            .header("x-aml-service-key", "bridge-fixture")
            .json(&body)
            .send()
    };
    let body = json!({"tx_hash":txid,"output_index":0});
    let stale: Value = request(body.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(stale["status"], "source_changed");
    assert!(
        store
            .bridge_traces(&id, &owner, now())
            .await?
            .unwrap()
            .is_empty()
    );
    changed.store(false, Ordering::SeqCst);
    let wrong: Value = request(json!({"tx_hash":txid,"output_index":1}))
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(wrong["status"], "anchor_not_found");
    assert_eq!(
        request(json!({"tx_hash":txid,"log_index":1}))
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let matched: Value = request(body).await?.error_for_status()?.json().await?;
    assert_eq!(matched["status"], "matched");
    let trace = &matched["trace"];
    assert_eq!(trace["claim"], "deposit_reveal");
    assert_eq!(trace["send"]["receipt"]["output_index"], 0);
    assert_eq!(trace["relay"]["receipt"]["log_index"], 76);
    assert_eq!(trace["mint_verified"], false);
    let retry: Value = request(json!({"tx_hash":txid}))
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(retry, matched);
    assert_eq!(
        store.bridge_traces(&id, &owner, now()).await?.unwrap(),
        vec![trace.clone()]
    );
    let body = json!({"tx_hash":txid,"output_index":0,"verify_sweep":true});
    let missing: Value = request(body.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(missing["status"], "sweep_proof_not_found");
    proof_available.store(true, Ordering::SeqCst);
    changed.store(true, Ordering::SeqCst);
    let stale: Value = request(body.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(stale["status"], "source_changed");
    assert_eq!(
        store.bridge_traces(&id, &owner, now()).await?.unwrap(),
        vec![trace.clone()]
    );
    changed.store(false, Ordering::SeqCst);
    let verified: Value = request(body.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(verified["status"], "matched");
    let sweep = &verified["trace"];
    assert_eq!(sweep["claim"], "deposit_sweep");
    assert_eq!(sweep["sweep_verified"], true);
    assert_eq!(sweep["mint_verified"], false);
    assert_eq!(sweep["send"]["receipt"]["input_index"], 1);
    assert_eq!(sweep["relay"]["receipt"]["log_index"], 752);
    assert_eq!(
        sweep["deposit"]["funding"]["receipt"],
        trace["send"]["receipt"]
    );
    assert_ne!(sweep["id"], trace["id"]);
    let retry: Value = request(body).await?.error_for_status()?.json().await?;
    assert_eq!(retry, verified);
    let saved = store.bridge_traces(&id, &owner, now()).await?.unwrap();
    assert_eq!(saved.len(), 2);
    assert!(saved.contains(sweep) && saved.contains(trace));
    task.abort();
    btc_task.abort();
    eth_task.abort();
    Ok(())
}

async fn adapter(
    network: Network,
    log: Log,
    change_status: Arc<AtomicBool>,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let tx_hash = log.tx_hash.clone();
    let block_number = log.block_number;
    let timestamp = log.timestamp_ms;
    let (side, key, operation_route) = if let Some(leg) = log.decode(network) {
        (leg.side, leg.operation_id, bridge::OPERATION_ROUTE)
    } else {
        use bridge::allbridge::Direction;
        let leg = bridge::allbridge::decode(network, &log).context("invalid test receipt")?;
        let (side, word) = match leg.direction {
            Direction::Send { .. } => (Side::Send, leg.nonce),
            Direction::Receive { message } => (Side::Relay, message),
        };
        (
            side,
            super::allbridge::hex(&word),
            bridge::ALLBRIDGE_OPERATION_ROUTE,
        )
    };
    let snapshot = json!({"network_id":network.id(),"contract":"canonical_v1","snapshot_id":"e".repeat(64),
        "initialized":true,"start_block":block_number,"as_of_block":block_number,"block_hash":log.block_hash});
    let status = snapshot.clone();
    let observe = move |hit: bool| {
        Json(json!({"network_id":network.id(),"source_snapshot":snapshot,
        "logs":if hit { vec![log.clone()] } else { Vec::new() },"truncated":false}))
    };
    let for_tx = observe.clone();
    serve(Router::new()
        .route(bridge::TRANSACTION_ROUTE, get(move |Path(hash): Path<String>, axum::extract::Query(anchor): axum::extract::Query<bridge::Anchor>| {
            let observation = for_tx(hash == tx_hash && anchor.block_number == block_number);
            async { observation }
        }))
        .route(operation_route, get(move |Path(hash): Path<String>, axum::extract::Query(window): axum::extract::Query<Window>| {
            let observation = observe(side == window.side && key == hash
                && (window.from_ms..=window.to_ms).contains(&timestamp));
            async { observation }
        }))
        .route(bridge::SNAPSHOT_ROUTE, get(move || {
            let mut snapshot = status.clone();
            if change_status.load(Ordering::SeqCst) { snapshot["snapshot_id"] = json!("f".repeat(64)); }
            async move { Json(snapshot) }
        }))).await
}

#[tokio::test]
#[ignore = "requires a disposable BRIDGE_NEO4J_URL and BRIDGE_NEO4J_PASSWORD"]
async fn cross_chain_bridge_http_persists_one_scoped_edge_on_neo4j() -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let store = Store::new(
        http.clone(),
        &std::env::var("BRIDGE_NEO4J_URL")?,
        std::env::var("BRIDGE_NEO4J_PASSWORD")?,
    );
    store.initialize().await?;
    let [eth_log, bsc_log]: [Log; 2] = match std::env::var_os("BRIDGE_RPC_FIXTURE") {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => [receipt(Network::Ethereum), receipt(Network::Bsc)],
    };
    persist_bridge_fixture(
        &store,
        &http,
        (Network::Ethereum, eth_log),
        (Network::Bsc, bsc_log),
        "celer_cbridge_pool_v2",
    )
    .await?;
    let (source, send, receive) = match std::env::var_os("ALLBRIDGE_RPC_FIXTURE") {
        Some(path) => {
            let (network, send, receive): (String, Log, Log) =
                serde_json::from_slice(&std::fs::read(path)?)?;
            (Network::parse(&network)?, send, receive)
        }
        None => {
            let (send, receive) = super::allbridge::tests::fixture(Network::Bsc);
            (Network::Bsc, send, receive)
        }
    };
    let destination = bridge::allbridge::peer(source).unwrap();
    persist_bridge_fixture(
        &store,
        &http,
        (source, send),
        (destination, receive),
        "allbridge_core",
    )
    .await
}

async fn persist_bridge_fixture(
    store: &Store,
    http: &reqwest::Client,
    source: (Network, Log),
    destination: (Network, Log),
    protocol: &str,
) -> anyhow::Result<()> {
    let (source_network, send) = source;
    let (destination_network, receive) = destination;
    let outside_tx = transaction_hash(destination_network, &"d".repeat(64))?;
    let tx = receive.tx_hash.clone();
    let block = receive.block_number;
    let index = receive.log_index;
    let change_status = Arc::new(AtomicBool::new(true));
    let (send, send_task) = adapter(source_network, send, change_status.clone()).await?;
    let (receive, receive_task) = adapter(
        destination_network,
        receive,
        Arc::new(AtomicBool::new(false)),
    )
    .await?;
    let (api, task) = serve(routes().with_state(App {
        store: store.clone(),
        http: http.clone(),
        key: "bridge-fixture".into(),
        ttl_ms: 60000,
        requests: Arc::new(tokio::sync::Semaphore::new(2)),
        upstreams: HashMap::from([
            (source_network.name().into(), send),
            (destination_network.name().into(), receive),
        ]),
    }))
    .await?;
    let id = nanoid::nanoid!(24);
    let expiry = now() + 60000;
    store.create(json!({"metadata":{"id":id,"state":"temporary","network":destination_network.name(),"network_id":destination_network.id(),"expires_at_unix_ms":expiry},
        "owner":user_owner_hash("bridge-owner"),
        "payload":json!({"graph":{"edges":[{"tx_hash":tx,"block_number":block}]}}).to_string(),"nodes":[],"edges":[]})).await?;
    let url = format!("{api}/api/investigations/{id}/bridges");
    let owner_request = |builder: reqwest::RequestBuilder| {
        builder
            .header("x-aml-user", "bridge-owner")
            .header("x-aml-service-key", "bridge-fixture")
    };
    let post = || {
        owner_request(http.post(&url))
            .json(&json!({"tx_hash":tx,"log_index":index}))
            .send()
    };
    assert_eq!(
        http.get(&url)
            .header("x-aml-user", "another")
            .header("x-aml-service-key", "bridge-fixture")
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        owner_request(http.post(&url))
            .json(&json!({"tx_hash":outside_tx}))
            .send()
            .await?
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let stale: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(stale["status"], "source_changed");
    let empty: Value = owner_request(http.get(&url))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(empty["traces"], json!([]));
    change_status.store(false, Ordering::SeqCst);
    let matched: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(matched["status"], "matched");
    assert_eq!(matched["trace"]["protocol"], protocol);
    assert_eq!(matched["trace"]["send"]["network_id"], source_network.id());
    assert_eq!(
        matched["trace"]["relay"]["network_id"],
        destination_network.id()
    );
    let retried: Value = post().await?.error_for_status()?.json().await?;
    assert_eq!(matched, retried);
    let reopened: Value = owner_request(http.get(&url))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(reopened["traces"], json!([matched["trace"]]));
    let edges = || {
        store.query("MATCH (:BridgeOccurrence)-[r:BRIDGE_OPERATION {id:$id}]->(:BridgeOccurrence) RETURN count(r)", json!({"id":matched["trace"]["id"]}))
    };
    assert_eq!(edges().await?, vec![json!([1])]);
    store.cleanup(expiry).await?;
    assert_eq!(edges().await?, vec![json!([0])]);
    task.abort();
    send_task.abort();
    receive_task.abort();
    Ok(())
}
