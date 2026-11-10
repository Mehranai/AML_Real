use super::*;
use crate::Store;
use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Notify, Semaphore};

#[test]
fn identity_and_rule_configuration_are_separate() {
    let base = json!({"network":"ethereum","kind":"transaction","value":"F".repeat(64),
        "system_id":"merchant-1","customer_id":"opaque:123"});
    let register =
        |value: Value, owner| registration(serde_json::from_value(value).unwrap(), owner);
    let initial = register(base.clone(), "operator").unwrap();
    assert_eq!(initial["value"], format!("0x{}", "f".repeat(64)));
    assert_eq!(initial["threshold"], 25);
    let mut changed = base.clone();
    changed["rule"] = json!("approved_label");
    changed["threshold"] = json!(70);
    assert_eq!(
        register(changed.clone(), "operator").unwrap()["id"],
        initial["id"]
    );
    changed["threshold"] = json!(71);
    assert!(register(changed, "operator").is_err());
    assert_ne!(
        register(base.clone(), "another").unwrap()["id"],
        initial["id"]
    );
    let mut changed = base.clone();
    changed["customer_id"] = json!("opaque:124");
    assert_ne!(register(changed, "operator").unwrap()["id"], initial["id"]);
    let mut changed = base;
    changed["network"] = json!("bsc");
    assert_ne!(register(changed, "operator").unwrap()["id"], initial["id"]);
    let bitcoin = json!({"network":"bitcoin","kind":"address","value":"1BoatSLRHtKNngkdXEeobR76b53LETtpyT",
        "system_id":"sys","customer_id":"customer"});
    assert!(register(bitcoin.clone(), "operator").is_ok());
    let mut bad_checksum = bitcoin;
    bad_checksum["value"] = json!("1BoatSLRHtKNngkdXEeobR76b53LETtpyU");
    assert!(register(bad_checksum, "operator").is_err());
}

pub(super) async fn serve(router: Router) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Ok((url, task))
}

pub(super) fn operator() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-aml-user", "fixture-operator".parse().unwrap());
    headers.insert("x-aml-service-key", "fixture-key".parse().unwrap());
    headers
}

#[tokio::test]
#[ignore = "requires a fresh disposable KYT_NEO4J_URL, KYT_NEO4J_PASSWORD and API3_PROOF_DIR"]
async fn watchlist_rechecks_four_chains_and_fences_inflight_jobs() -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let store = Store::new(
        http.clone(),
        &std::env::var("KYT_NEO4J_URL")?,
        std::env::var("KYT_NEO4J_PASSWORD")?,
    );
    store.initialize().await?;
    let directory = std::env::var("API3_PROOF_DIR")?;
    let headers = operator();
    let mut other = headers.clone();
    other.insert("x-aml-user", "fixture-other-operator".parse()?);
    for network in Network::ALL {
        let fixture: Value = serde_json::from_slice(&std::fs::read(format!(
            "{directory}/{}.json",
            network.name()
        ))?)?;
        let wallet = fixture["wallet"].clone();
        let address = wallet["address"]
            .as_str()
            .context("fixture wallet address")?;
        let source = &wallet["source_snapshot"];
        let wallet_state = Arc::new(RwLock::new(wallet.clone()));
        let hash = kyt::transaction_hash(network, &"d".repeat(64))?;
        let observation = Arc::new(RwLock::new(
            json!({"network_id":network.id(),"tx_hash":hash,
            "source_snapshot":source,"ambiguous":false,"occurrence":null}),
        ));
        let hold = Arc::new(AtomicBool::new(false));
        let requested = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut adapter = Router::new()
            .route(
                kyt::TRANSACTION_ROUTE,
                get({
                    let observation = observation.clone();
                    move || {
                        let data = observation.read().unwrap().clone();
                        async { Json(data) }
                    }
                }),
            )
            .route(
                &format!("/api/{}/wallet/{{address}}/investigation", network.name()),
                get({
                    let (hold, requested, release) =
                        (hold.clone(), requested.clone(), release.clone());
                    let wallet = wallet_state.clone();
                    move || {
                        let (hold, requested, release, wallet) = (
                            hold.clone(),
                            requested.clone(),
                            release.clone(),
                            wallet.clone(),
                        );
                        async move {
                            if hold.swap(false, Ordering::SeqCst) {
                                requested.notify_one();
                                release.notified().await;
                            }
                            Json(wallet.read().unwrap().clone())
                        }
                    }
                }),
            );
        for (path, value) in fixture["receipts"]
            .as_object()
            .context("missing receipts")?
        {
            let value = value.clone();
            let current =
                (path == "/status" && network == Network::Ethereum).then(|| wallet_state.clone());
            adapter = adapter.route(
                path,
                get(move || {
                    let mut value = value.clone();
                    if let Some(wallet) = &current {
                        value["operational_snapshot"] =
                            wallet.read().unwrap()["operational_snapshot"].clone();
                    }
                    async { Json(value) }
                }),
            );
        }
        let (adapter_url, adapter_task) = serve(adapter).await?;
        let app = App {
            store: store.clone(),
            http: http.clone(),
            upstreams: HashMap::from([(network.name().into(), adapter_url)]),
            key: "fixture-key".into(),
            ttl_ms: 1_000,
            requests: Arc::new(Semaphore::new(2)),
        };
        let (url, api_task) = serve(routes().with_state(app.clone())).await?;
        let watchlist = format!("{url}/api/kyt/watchlist");
        let input = json!({"network":network.name(),"kind":"address","value":address,
            "system_id":"fixture","customer_id":"opaque:001","threshold":1});
        let register = |input: Value| http.post(&watchlist).headers(headers.clone()).json(&input);
        let created: Value = register(input.clone())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let id = &created["id"];
        let entry_url = format!("{watchlist}/{}", id.as_str().unwrap());
        let read = || http.get(&entry_url).headers(headers.clone());
        assert!(created["result"].is_null());
        assert_eq!(
            http.get(&entry_url)
                .headers(other.clone())
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            http.delete(&entry_url)
                .headers(other.clone())
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            http.get(&entry_url)
                .header("x-aml-service-key", "fixture-key")
                .send()
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let listed: Value = http
            .get(&watchlist)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(listed["entries"][0]["id"], *id);
        check_due(&app, false).await?;
        let checked: Value = read().send().await?.error_for_status()?.json().await?;
        assert_eq!(
            checked["result"]["source_snapshot_id"],
            source["snapshot_id"],
            "{}",
            network.name()
        );
        assert!(checked["last_checked_at_ms"].is_number());
        if checked["result"]["status"] == "matched" {
            assert!(checked["match_alert_id"].as_str().is_some_and(watch_id));
        }
        let repeated: Value = register(input.clone())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(repeated["result"], checked["result"]);
        assert_eq!(repeated["next_check_at_ms"], checked["next_check_at_ms"]);

        if network == Network::Ethereum {
            assert_eq!(checked["result"]["status"], "matched");
            let mut input = input;
            input["threshold"] = json!(99);
            let _: Value = register(input.clone())
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            hold.store(true, Ordering::SeqCst);
            let in_flight = tokio::spawn({
                let app = app.clone();
                async move { check_due(&app, false).await }
            });
            tokio::time::timeout(Duration::from_secs(10), requested.notified()).await?;
            input["threshold"] = json!(100);
            let _: Value = register(input.clone())
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            release.notify_one();
            in_flight.await??;
            let changed: Value = read().send().await?.error_for_status()?.json().await?;
            assert!(
                changed["result"].is_null(),
                "old worker must not publish under a changed rule"
            );
            check_due(&app, false).await?;
            let checked: Value = read().send().await?.error_for_status()?.json().await?;
            assert_eq!(checked["result"]["status"], "no_match");
            input["threshold"] = json!(1);
            input["rule"] = json!("approved_label");
            let _: Value = register(input)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            check_due(&app, false).await?;
            let approved: Value = read().send().await?.error_for_status()?.json().await?;
            assert_eq!(approved["result"]["status"], "matched");
            {
                let mut changed = wallet_state.write().unwrap();
                changed.as_object_mut().unwrap().remove("response_contract");
                changed["entities"][0]["review_status"] = json!("revoked");
                changed["operational_snapshot"]["snapshot_id"] = json!("e".repeat(64));
            }
            store
                .query(
                    "MATCH (w:KytWatch {id:$id}) SET w.next_check_at_ms=0 RETURN w.id",
                    json!({"id":id}),
                )
                .await?;
            check_due(&app, false).await?;
            let revoked: Value = read().send().await?.error_for_status()?.json().await?;
            assert_eq!(revoked["result"]["status"], "no_match");
            assert_eq!(
                revoked["result"]["source_snapshot_id"],
                approved["result"]["source_snapshot_id"]
            );
            *wallet_state.write().unwrap() = wallet.clone();
        }
        http.delete(&entry_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?;
        assert_eq!(read().send().await?.status(), StatusCode::NOT_FOUND);
        let transaction: Value = register(
            json!({"network":network.name(),"kind":"transaction","value":hash,
            "system_id":"fixture","customer_id":"opaque:001","threshold":1}),
        )
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
        let tx_id = transaction["id"].as_str().unwrap();
        let tx_url = format!("{watchlist}/{tx_id}");
        let read_tx = || http.get(&tx_url).headers(headers.clone());
        check_due(&app, false).await?;
        let missing: Value = read_tx().send().await?.error_for_status()?.json().await?;
        assert_eq!(
            missing["result"]["status"],
            "not_indexed",
            "{}",
            network.name()
        );
        observation.write().unwrap()["occurrence"] = json!({"block_number":source["as_of_block"],
            "block_hash":source["block_hash"],"participants":[address],"participants_truncated":false});
        store
            .query(
                "MATCH (w:KytWatch {id:$id}) SET w.next_check_at_ms=0 RETURN w.id",
                json!({"id":tx_id}),
            )
            .await?;
        check_due(&app, false).await?;
        let indexed: Value = read_tx().send().await?.error_for_status()?.json().await?;
        assert_eq!(
            indexed["result"]["status"],
            checked["result"]["status"],
            "{}",
            network.name()
        );
        assert_eq!(indexed["result"]["block_hash"], source["block_hash"]);
        http.delete(&tx_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?;
        eprintln!(
            "KYT real Neo4j: {} register/read/owner/recheck/canonical receipt passed",
            network.name()
        );
        api_task.abort();
        adapter_task.abort();
    }
    Ok(())
}
