use super::*;
use std::sync::{Arc, RwLock};
async fn decoded(response: Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}
#[tokio::test]
#[ignore = "requires TRON_OPERATIONS_PROOF from live TRON ClickHouse acceptance"]
async fn shared_tron_evidence_reaches_central_create_reopen_and_export() {
    let proof: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("TRON_OPERATIONS_PROOF").unwrap()).unwrap(),
    )
    .unwrap();
    let direct = proof["direct"].clone();
    assert_eq!(
        direct["response_contract"], "chain_response_v1",
        "regenerate TRON_OPERATIONS_PROOF with the current adapter"
    );
    let address = direct["address"].as_str().unwrap().to_owned();
    let assessed = risk::assess("tron", &direct);
    assert!(
        assessed["risk_score"].as_f64().is_some_and(|v| v > 0.0),
        "{assessed}"
    );
    assert!(
        risk::assess("tron", &proof["revoked"])["signals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let captured = Arc::new(RwLock::new(Value::Null));
    let stored = captured.clone();
    let neo = post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            let statement = body["statement"].as_str().unwrap();
            let parameters = &body["parameters"];
            let rows = if statement.contains("CREATE (i:Investigation)") {
                *captured.write().unwrap() = parameters.clone();
                json!([[parameters["metadata"]["id"], 0, 0]])
            } else {
                let mut saved = captured.write().unwrap();
                if statement.contains("RETURN i.payload_json") {
                    json!([[saved["payload"], saved["metadata"]]])
                } else {
                    saved["metadata"]["state"] = json!("saved");
                    json!([[saved["metadata"]]])
                }
            };
            Json(json!({"data":{"values":rows},"errors":[]}))
        }
    });
    let current = Arc::new(RwLock::new(
        json!({"network_id":"tron:mainnet","source_snapshot":direct["source_snapshot"],"operational_snapshot":direct["operational_snapshot"]}),
    ));
    let status = current.clone();
    let upstream = direct.clone();
    let router = Router::new()
        .route(
            "/status",
            get(move || {
                let data = status.read().unwrap().clone();
                async move { Json(data) }
            }),
        )
        .route(
            "/api/tron/wallet/{address}/investigation",
            get(move || {
                let data = upstream.clone();
                async move { Json(data) }
            }),
        )
        .route("/db/neo4j/query/v2", neo);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let http = reqwest::Client::new();
    let app = App {
        store: Store::new(http.clone(), &url, String::new()),
        http,
        upstreams: HashMap::from([("tron".into(), url)]),
        key: "fixture".into(),
        ttl_ms: 100000,
        requests: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-aml-service-key", "fixture".parse().unwrap());
    headers.insert("x-aml-user", "fixture-reviewer".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let created = decoded(
        create(
            app.clone(),
            headers.clone(),
            "tron".into(),
            address.clone(),
            None,
            format!("/api/tron/wallet/{address}/investigation")
                .parse()
                .unwrap(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(created["source_validity"]["status"], "current");
    assert_eq!(created["risk_engine"], assessed);
    let saved: Value =
        serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
    assert_eq!(
        saved["operational_snapshot"],
        direct["operational_snapshot"]
    );
    assert_eq!(saved["exposure"], direct["exposure"]);
    assert!(
        !stored.read().unwrap()["edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let id = created["investigation"]["id"].as_str().unwrap().to_owned();
    let reopened = decoded(
        reopen(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(reopened["source_validity"]["status"], "current");
    // Revocation without a new blockchain block still invalidates the saved current risk.
    current.write().unwrap()["operational_snapshot"] =
        proof["revoked"]["operational_snapshot"].clone();
    let stale = decoded(
        reopen(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(stale["source_validity"]["status"], "stale");
    assert!(stale["risk_engine"]["risk_score"].is_null());
    assert_eq!(stale["historical_risk_engine"], assessed);
    let exported = decoded(
        export(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(exported["source_validity"]["status"], "stale");
    assert_eq!(exported["investigation"]["state"], "saved");
    assert_eq!(exported["historical_risk_engine"], assessed);
    let retained: Value =
        serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
    assert_eq!(retained["exposure"], direct["exposure"]);
    current.write().unwrap()["network_id"] = json!("eip155:1");
    let unavailable = decoded(reopen(State(app), headers, Path(id)).await.unwrap()).await;
    assert_eq!(unavailable["source_validity"]["status"], "unverified");
    assert!(unavailable["risk_engine"]["risk_score"].is_null());
    server.abort();
}

#[test]
fn missing_tron_operational_proof_cannot_keep_current_risk() {
    let mut data = json!({"network_id":"tron:mainnet","risk_engine":{"risk_score":90}});
    apply_tron_source_validity(&mut data, None);
    assert_eq!(data["source_validity"]["status"], "unverified");
    assert!(data["risk_engine"]["risk_score"].is_null());
}
