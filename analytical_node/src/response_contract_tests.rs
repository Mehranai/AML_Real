//! Adapter HTTP payloads captured by the disposable ClickHouse acceptance tests.
//! This acceptance uses the real Store and a disposable Neo4j, never deployment data.
use super::*;
use std::sync::RwLock;

async fn decode(response: Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn account_freshness_fences_chain_and_operational_evidence() {
    for (network, network_id) in [("ethereum", "eip155:1"), ("bsc", "eip155:56")] {
        let mut base = json!({"network_id":network_id,
            "source_snapshot":{"contract":"canonical_v1","network_id":network_id,"snapshot_id":"a".repeat(64)},
            "operational_snapshot":{"contract":"operational_v1","network_id":network_id,"snapshot_id":"b".repeat(64)},
            "fingerprint":{"transfer_count":3}});
        let assessed = risk::assess(network, &base);
        risk::attach_assessment(&mut base, assessed);
        let mut current = base.clone();
        apply_account_source_validity(&mut current, Some(&base), network);
        assert_eq!(current["source_validity"]["status"], "current");
        assert_eq!(current["source_validity"]["current_risk_usable"], true);
        for field in ["source_snapshot", "operational_snapshot"] {
            let mut live = base.clone();
            live[field]["snapshot_id"] = json!("c".repeat(64));
            let mut stale = base.clone();
            apply_account_source_validity(&mut stale, Some(&live), network);
            assert_eq!(stale["source_validity"]["status"], "stale");
            assert_eq!(stale["risk_engine"]["risk_level"], "UNKNOWN");
            assert_eq!(stale["historical_risk_engine"], base["risk_engine"]);
        }
        apply_account_source_validity(&mut current, None, network);
        assert_eq!(current["source_validity"]["status"], "unverified");
        let mut paths = base.clone();
        paths.as_object_mut().unwrap().remove("risk_engine");
        apply_account_source_validity(&mut paths, Some(&base), network);
        assert_eq!(paths["source_validity"]["current_risk_usable"], false);
    }
}

#[tokio::test]
#[ignore = "requires API3_PROOF_DIR from four live adapter tests and disposable API3_NEO4J_URL"]
async fn four_chain_responses_survive_real_neo4j_create_reopen_export() -> anyhow::Result<()> {
    let directory = std::env::var("API3_PROOF_DIR")?;
    let url = std::env::var("API3_NEO4J_URL")?;
    let password = std::env::var("API3_NEO4J_PASSWORD")?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let store = Store::new(http.clone(), &url, password);
    store.initialize().await?;
    let live = Arc::new(RwLock::new(Value::Null));
    let read = live.clone();
    let router = Router::new().fallback(get(move |OriginalUri(uri): OriginalUri| {
        let fixture = read.read().unwrap().clone();
        async move {
            if let Some(value) = fixture["receipts"].get(uri.path()) {
                return Json(value.clone()).into_response();
            }
            if uri.path().ends_with("/investigation") {
                Json(fixture["wallet"].clone()).into_response()
            } else if uri.path().contains("/paths/") {
                Json(fixture["paths"].clone()).into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error":"missing captured receipt"})),
                )
                    .into_response()
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let app = App {
        store: store.clone(),
        http,
        upstreams: ["bitcoin", "ethereum", "bsc", "tron"]
            .into_iter()
            .map(|n| (n.to_owned(), upstream.clone()))
            .collect(),
        key: "api3-fixture-service-key-0123456789abcdef".into(),
        ttl_ms: 3_600_000,
        requests: Arc::new(tokio::sync::Semaphore::new(2)),
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-aml-service-key", app.key.parse()?);
    headers.insert("x-aml-user", "api3-fixture-analyst".parse()?);
    headers.insert("content-type", "application/json".parse()?);
    for network in ["bitcoin", "ethereum", "bsc", "tron"] {
        let fixture: Value =
            serde_json::from_slice(&std::fs::read(format!("{directory}/{network}.json"))?)?;
        *live.write().unwrap() = fixture.clone();
        for mode in ["wallet", "paths"] {
            let expected = &fixture[mode];
            let address = expected["address"]
                .as_str()
                .context("captured address missing")?
                .to_owned();
            let target = (mode == "paths").then(|| expected["target"].as_str().unwrap().to_owned());
            let route = target
                .as_ref()
                .map(|v| format!("/api/{network}/wallet/{address}/paths/{v}"))
                .unwrap_or_else(|| format!("/api/{network}/wallet/{address}/investigation"));
            let created = decode(
                create(
                    app.clone(),
                    headers.clone(),
                    network.into(),
                    address.clone(),
                    target.clone(),
                    route.parse()?,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{network}/{mode} create: {e:?}"))?,
            )
            .await;
            assert_eq!(
                created["response_contract"], "chain_response_v1",
                "{network}/{mode}"
            );
            assert_eq!(
                created["source_validity"]["status"], "current",
                "{network}/{mode}: {}",
                created["source_validity"]
            );
            assert_eq!(created["evidence"], expected["evidence"]);
            assert_eq!(created["graph"], expected["graph"]);
            assert_eq!(
                created["semantic_evidence"]["contract"],
                "chain_semantic_evidence_v1"
            );
            assert_eq!(
                created["semantic_evidence"]["source_snapshot_id"],
                created["source_snapshot"]["snapshot_id"]
            );
            if mode == "paths" {
                assert_eq!(created["semantic_evidence"]["scope"], "not_requested");
            }
            let id = created["investigation"]["id"].as_str().unwrap().to_owned();
            let reopened = decode(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(reopened["graph"], expected["graph"]);
            assert_eq!(reopened["evidence"], expected["evidence"]);
            assert_eq!(reopened["semantic_evidence"], created["semantic_evidence"]);
            if network == "bitcoin" && mode == "wallet" {
                assert_eq!(reopened["holdings"], expected["holdings"]);
            }
            let projected = store.query("MATCH ()-[r:TRANSFER {investigation_id:$id}]->() RETURN r.edge_id,r.amount,r.asset_id,r.token_id,r.transaction_index,r.event_index,r.event_sub_index",json!({"id":id})).await?;
            assert_eq!(
                projected.len(),
                expected["graph"]["edges"].as_array().unwrap().len()
            );
            for row in projected {
                let edge = expected["graph"]["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|e| e["id"] == row[0])
                    .unwrap();
                for (column, field) in [
                    (1, "amount_raw"),
                    (2, "canonical_asset_id"),
                    (3, "canonical_token_id"),
                    (4, "transaction_index"),
                    (5, "event_index"),
                    (6, "event_sub_index"),
                ] {
                    assert_eq!(row[column], edge[field], "{network}: projected {field}");
                }
            }
            let invalid_nodes = store.query("MATCH (w:InvestigationWallet {investigation_id:$id}) WHERE w.subject_type <> 'address' AND w.address IS NOT NULL RETURN count(w)",json!({"id":id})).await?;
            assert_eq!(invalid_nodes[0][0], 0);
            let mut outsider = headers.clone();
            outsider.insert("x-aml-user", "another-analyst".parse()?);
            assert_eq!(
                reopen(State(app.clone()), outsider, Path(id.clone()))
                    .await
                    .err()
                    .unwrap()
                    .0,
                StatusCode::NOT_FOUND
            );
            let saved = decode(
                export(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(saved["investigation"]["state"], "saved");
            assert_eq!(saved["source_validity"]["status"], "current");
            if mode == "wallet" {
                assert_eq!(
                    created["risk_policy_hash"],
                    aml_risk_policy::digest(&aml_risk_policy::definition())
                );
                for response in [&created, &reopened, &saved] {
                    assert_eq!(response["risk_policy_hash"], created["risk_policy_hash"]);
                    assert_eq!(response["risk_engine"], created["risk_engine"]);
                    assert_eq!(
                        response["source_validity"]["current_risk_usable"],
                        response["risk_engine"]["risk_score"].is_number()
                    );
                }
            } else {
                assert!(created.get("risk_engine").is_none());
                assert!(saved.get("risk_policy_hash").is_none());
            }
            let persisted = store
                .query(
                    "MATCH (i:Investigation {id:$id}) RETURN i.payload_json",
                    json!({"id":id}),
                )
                .await?;
            if network != "bitcoin" {
                live.write().unwrap()["receipts"]["/status"]["operational_snapshot"]["snapshot_id"] =
                    json!("e".repeat(64));
                let changed = decode(
                    reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                        .await
                        .unwrap(),
                )
                .await;
                assert_eq!(changed["source_validity"]["status"], "stale");
                assert_eq!(changed["evidence"], expected["evidence"]);
                *live.write().unwrap() = fixture.clone();
            }
            live.write().unwrap()["receipts"]["/status"]["source_snapshot"]["snapshot_id"] =
                json!("f".repeat(64));
            let stale = decode(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(stale["source_validity"]["status"], "stale");
            assert_eq!(stale["evidence"], expected["evidence"]);
            assert_eq!(stale["graph"], expected["graph"]);
            assert_eq!(stale["semantic_evidence"], created["semantic_evidence"]);
            if mode == "wallet" {
                assert_eq!(stale["risk_engine"]["risk_level"], "UNKNOWN");
                assert_eq!(stale["historical_risk_engine"], created["risk_engine"]);
            } else {
                assert_eq!(stale["source_validity"]["current_risk_usable"], false);
            }
            let stale_export = decode(
                export(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(stale_export["source_validity"]["status"], "stale");
            assert_eq!(
                stale_export["source_validity"]["current_risk_usable"],
                false
            );
            if mode == "wallet" {
                assert_eq!(
                    stale_export["risk_policy_hash"],
                    created["risk_policy_hash"]
                );
                assert_eq!(
                    stale_export["historical_risk_engine"],
                    created["risk_engine"]
                );
            }
            assert_eq!(
                store
                    .query(
                        "MATCH (i:Investigation {id:$id}) RETURN i.payload_json",
                        json!({"id":id})
                    )
                    .await?,
                persisted
            );
            *live.write().unwrap() = fixture.clone();
            store.query("MATCH (i:Investigation {id:$id}) OPTIONAL MATCH (i)-[:CONTAINS]->(w) DETACH DELETE w,i",json!({"id":id})).await?;
            eprintln!(
                "API-3 real Neo4j: {network}/{mode} create/reopen/export/stale/owner/projection passed"
            );
        }
        let address = fixture["wallet"]["address"].as_str().unwrap().to_owned();
        live.write().unwrap()["wallet"]["evidence"]["holdings"]["model"] =
            json!("fabricated_balance");
        assert_eq!(
            create(
                app.clone(),
                headers.clone(),
                network.into(),
                address.clone(),
                None,
                format!("/api/{network}/wallet/{address}/investigation").parse()?
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::BAD_GATEWAY
        );
    }
    server.abort();
    Ok(())
}
