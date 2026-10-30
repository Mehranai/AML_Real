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
#[ignore = "requires Bitcoin exposure ClickHouse acceptance JSON"]
async fn canonical_bitcoin_exposure_reaches_risk_create_reopen_and_export() {
    let file = std::env::var("BITCOIN_EXPOSURE_ACCEPTANCE").unwrap();
    let proof: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let mut report = proof.clone();
    for name in ["direct", "indirect", "script", "outgoing", "grouped"] {
        let assessed = risk::assess("bitcoin", &proof[name]);
        assert!(
            assessed["risk_score"].as_f64().is_some_and(|n| n > 0.0),
            "{name}: {assessed}"
        );
        assert_eq!(
            assessed["signals"][0]["signal_type"],
            "ILLICIT_SEED_EXPOSURE"
        );
        report[name]["risk_engine"] = assessed;
    }
    for name in [
        "mixed",
        "unrelated",
        "service",
        "behind_service",
        "after_revocation",
    ] {
        let assessed = risk::assess("bitcoin", &proof[name]);
        assert!(assessed["risk_score"].is_null(), "{name}: {assessed}");
        report[name]["risk_engine"] = assessed;
    }
    let mut duplicate = proof["direct"].clone();
    let paths = duplicate["exposure_paths"].as_array().unwrap().clone();
    duplicate["exposure_paths"]
        .as_array_mut()
        .unwrap()
        .extend(paths);
    assert_eq!(
        risk::assess("bitcoin", &duplicate)["risk_score"],
        report["direct"]["risk_engine"]["risk_score"]
    );
    for change in [
        "bounds",
        "method",
        "seed",
        "outpoint",
        "snapshot",
        "network",
        "ownership",
    ] {
        let mut bad = proof["indirect"].clone();
        for p in bad["exposure_paths"].as_array_mut().unwrap() {
            match change {
                "bounds" => p["allocation"]["steps"][0]["lower_output_sats"] = json!("999"),
                "method" => p["attribution_method"] = json!("poison_every_output"),
                "seed" => p["seed_claim"]["active"] = json!(false),
                "outpoint" => p["hops"][1]["inputs"][0]["prev_tx_hash"] = json!("0".repeat(64)),
                "network" => p["network_id"] = json!("eip155:1"),
                "ownership" => p["ownership_claimed"] = json!(true),
                _ => {}
            }
        }
        if change == "snapshot" {
            bad["exposure_snapshot"] = Value::Null;
        }
        assert!(
            risk::assess("bitcoin", &bad)["risk_score"].is_null(),
            "tampered {change} was scored"
        );
    }
    let direct = proof["direct"].clone();
    assert_eq!(
        direct["risk_snapshot"]["policy_hash"],
        aml_risk_policy::digest(&aml_risk_policy::definition())
    );
    assert_eq!(
        direct["risk_snapshot"]["evidence_hash"],
        aml_risk_policy::digest(&aml_risk_policy::bitcoin_inputs(&direct))
    );
    assert_eq!(
        direct["risk_snapshot"]["risk_engine_hash"],
        aml_risk_policy::digest(&risk::assess("bitcoin", &direct))
    );
    assert_eq!(direct["risk_engine"], risk::assess("bitcoin", &direct));
    let network = direct["network_id"].clone();
    let address = direct["address"].as_str().unwrap().to_owned();
    let live = Arc::new(RwLock::new(direct["exposure_snapshot"].clone()));
    assert!(
        direct["risk_snapshot"].is_object(),
        "producer must persist its assessment"
    );
    let risk_live = Arc::new(RwLock::new(direct["risk_snapshot"].clone()));
    let risk_read = risk_live.clone();
    let risk_network = network.clone();
    let stored = Arc::new(RwLock::new(Value::Null));
    let captured = stored.clone();
    let neo = post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            let statement = body["statement"].as_str().unwrap();
            let parameters = &body["parameters"];
            let rows = if statement.contains("CREATE (i:Investigation)") {
                *captured.write().unwrap() = parameters.clone();
                json!([[parameters["metadata"]["id"], 0, 0]])
            } else {
                let mut data = captured.write().unwrap();
                if statement.contains("RETURN i.payload_json") {
                    json!([[data["payload"], data["metadata"]]])
                } else {
                    data["metadata"]["state"] = json!("saved");
                    json!([[data["metadata"]]])
                }
            };
            Json(json!({"data":{"values":rows},"errors":[]}))
        }
    });
    let status = Arc::new(RwLock::new(
        json!({"network_id":network,"source_snapshot":direct["source_snapshot"],"native_asset_snapshot":direct["holdings"]["native_asset_snapshot"]}),
    ));
    let intel = Arc::new(RwLock::new(
        json!({"network_id":network,"intelligence_snapshot":direct["intelligence_snapshot"]}),
    ));
    let upstream = Arc::new(RwLock::new(direct.clone()));
    let status_read = status.clone();
    let intel_read = intel.clone();
    let upstream_read = upstream.clone();
    let cluster_read = upstream.clone();
    let cluster_override = Arc::new(RwLock::new(None::<Value>));
    let cluster_override_read = cluster_override.clone();
    let read = live.clone();
    let n = network.clone();
    let semantic_live = Arc::new(RwLock::new(
        json!({"contract":"bitcoin_semantics_config_v1","network_id":network,"detector_version":direct["semantic_snapshot"]["detector_version"],"definition_hash":direct["semantic_snapshot"]["definition_hash"]}),
    ));
    let semantic_read = semantic_live.clone();
    let router = Router::new()
        .route("/api/bitcoin/wallet/{address}/clusters/{run}/snapshot",get(move || {
            let data=cluster_read.read().unwrap().clone();
            let value=cluster_override_read.read().unwrap().clone().unwrap_or_else(||json!({"network_id":data["network_id"],"cluster_snapshot":data["cluster_snapshot"],
                "cluster_config":{"contract":"bitcoin_clustering_config_v1","network_id":data["network_id"],"detector_version":data["cluster_snapshot"]["detector_version"],"definition_hash":data["cluster_snapshot"]["definition_hash"]}}));
            async move {Json(value)}
        }))
        .route(
            "/api/bitcoin/semantics/config",
            get(move || {
                let data = semantic_read.read().unwrap().clone();
                async move { Json(data) }
            }),
        )
        .route(
            "/status",
            get(move || {
                let data = status_read.read().unwrap().clone();
                async move { Json(data) }
            }),
        )
        .route(
            "/api/bitcoin/wallet/{address}/investigation",
            get(
                move |axum::extract::OriginalUri(uri): axum::extract::OriginalUri| {
                    let data = upstream_read.read().unwrap().clone();
                    async move {
                        if data["fixture_scope_error"] == true {
                            return (
                                StatusCode::UNPROCESSABLE_ENTITY,
                                Json(json!({"error":"fixture internal query details"})),
                            )
                                .into_response();
                        }
                        if data["query_scope"]["analytics"]["filters"]["from_block"] == 2 {
                            let query = uri.query().unwrap();
                            for expected in ["from_block=2", "to_block=2", "direction=spent"] {
                                assert!(query.contains(expected), "central lost {expected}");
                            }
                        }
                        Json(data).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/bitcoin/wallet/{address}/intelligence/snapshot",
            get(move || {
                let data = intel_read.read().unwrap().clone();
                async move { Json(data) }
            }),
        )
        .route(
            "/api/bitcoin/wallet/{address}/exposure/{run}/snapshot",
            get(move || {
                let frame = read.read().unwrap().clone();
                let n = n.clone();
                async move { Json(json!({"network_id":n,"exposure_snapshot":frame})) }
            }),
        )
        .route("/db/neo4j/query/v2", neo);
    let router = router.route(
        "/api/bitcoin/wallet/{address}/risk/{assessment}/snapshot",
        get(move || {
            let frame = risk_read.read().unwrap().clone();
            let network = risk_network.clone();
            async move { Json(json!({"network_id":network,"risk_snapshot":frame})) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let http = reqwest::Client::new();
    let app = App {
        store: Store::new(http.clone(), &url, String::new()),
        http,
        upstreams: HashMap::from([("bitcoin".into(), url)]),
        key: "fixture".into(),
        ttl_ms: 100000,
        requests: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-aml-service-key", "fixture".parse().unwrap());
    headers.insert("x-aml-user", "fixture-user".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let created = decoded(
        create(
            app.clone(),
            headers.clone(),
            "bitcoin".into(),
            address.clone(),
            None,
            format!("/api/bitcoin/wallet/{address}/investigation?limit=40")
                .parse()
                .unwrap(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(created["source_validity"]["status"], "current");
    assert!(created["risk_engine"]["risk_score"].as_f64().unwrap() > 0.0);
    let id = created["investigation"]["id"].as_str().unwrap().to_owned();
    let persisted: Value =
        serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
    assert_eq!(persisted["exposure_paths"], direct["exposure_paths"]);
    assert_eq!(persisted["risk_snapshot"], direct["risk_snapshot"]);
    risk_live.write().unwrap()["current"] = json!(false);
    let stale_risk = decoded(
        reopen(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(stale_risk["source_validity"]["status"], "stale");
    assert!(stale_risk["risk_engine"]["risk_score"].is_null());
    assert_eq!(stale_risk["historical_risk_engine"], created["risk_engine"]);
    *risk_live.write().unwrap() = Value::Null;
    let missing_risk = decoded(
        reopen(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(missing_risk["source_validity"]["status"], "unverified");
    *risk_live.write().unwrap() = direct["risk_snapshot"].clone();
    *live.write().unwrap() = proof["source_inactive"]["exposure_snapshot"].clone();
    let historical = decoded(
        reopen(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(historical["source_validity"]["status"], "stale");
    assert!(historical["risk_engine"]["risk_score"].is_null());
    assert_eq!(historical["historical_risk_engine"], created["risk_engine"]);
    assert_eq!(historical["exposure_paths"], direct["exposure_paths"]);
    let exported = decoded(
        export(State(app.clone()), headers.clone(), Path(id.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(exported["source_validity"]["status"], "stale");
    assert!(exported["risk_engine"]["risk_score"].is_null());
    *live.write().unwrap() = Value::Null;
    let unknown = decoded(
        reopen(State(app.clone()), headers.clone(), Path(id))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(unknown["source_validity"]["status"], "unverified");
    report["historical"] = historical;
    report["created"] = created;
    report["stale_risk"] = stale_risk;
    if let Ok(file) = std::env::var("BITCOIN_ACTIVITY_ACCEPTANCE") {
        let fixture: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let scoped = &fixture["scoped"];
        *upstream.write().unwrap() = scoped.clone();
        *status.write().unwrap() = json!({"network_id":network,"source_snapshot":scoped["source_snapshot"],"native_asset_snapshot":scoped["holdings"]["native_asset_snapshot"]});
        *intel.write().unwrap() =
            json!({"network_id":network,"intelligence_snapshot":scoped["intelligence_snapshot"]});
        *live.write().unwrap() = scoped["exposure_snapshot"].clone();
        *risk_live.write().unwrap() = scoped["risk_snapshot"].clone();
        *semantic_live.write().unwrap() = json!({"contract":"bitcoin_semantics_config_v1","network_id":network,"detector_version":scoped["semantic_snapshot"]["detector_version"],"definition_hash":scoped["semantic_snapshot"]["definition_hash"]});
        let created=decoded(create(app.clone(),headers.clone(),"bitcoin".into(),address.clone(),None,
            format!("/api/bitcoin/wallet/{address}/investigation?from_block=2&to_block=2&direction=spent").parse().unwrap()).await.unwrap()).await;
        assert_eq!(created["source_validity"]["status"], "current");
        assert_eq!(created["fingerprint"]["net_flow_sats"], "-51");
        assert_eq!(created["holdings"]["balance_sats"], "9");
        let saved: Value =
            serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
        let id = created["investigation"]["id"].as_str().unwrap().to_owned();
        let reopened = decoded(
            reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                .await
                .unwrap(),
        )
        .await;
        let exported = decoded(
            export(State(app.clone()), headers.clone(), Path(id.clone()))
                .await
                .unwrap(),
        )
        .await;
        let after_export = decoded(
            reopen(State(app.clone()), headers.clone(), Path(id))
                .await
                .unwrap(),
        )
        .await;
        for value in [&saved, &reopened, &after_export] {
            for field in [
                "fingerprint",
                "activity",
                "recent_transactions",
                "top_counterparties",
                "asset_flows",
                "query_scope",
                "activity_coverage",
            ] {
                assert_eq!(value[field], scoped[field], "lost scoped {field}");
            }
            assert_eq!(value["risk_engine"]["risk_level"], "UNKNOWN");
        }
        assert_eq!(exported["investigation"]["state"], "saved");
        assert_eq!(after_export["investigation"]["state"], "saved");
        upstream.write().unwrap()["fixture_scope_error"] = json!(true);
        let error = create(
            app.clone(),
            headers.clone(),
            "bitcoin".into(),
            address.clone(),
            None,
            format!("/api/bitcoin/wallet/{address}/investigation")
                .parse()
                .unwrap(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(error.1.contains("narrow"));
        report["scoped_activity_created"] = created;
        report["scoped_activity_exported"] = after_export;
        report["scoped_activity_export_receipt"] = exported;
    }
    if let Ok(file) = std::env::var("BITCOIN_SEMANTIC_ACCEPTANCE") {
        let fixture: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        for name in ["patterns", "coinjoin"] {
            let data = &fixture[name];
            let subject = data["address"].as_str().unwrap();
            *upstream.write().unwrap() = data.clone();
            *status.write().unwrap() = json!({"network_id":network,"source_snapshot":data["source_snapshot"],"native_asset_snapshot":data["holdings"]["native_asset_snapshot"]});
            *intel.write().unwrap() =
                json!({"network_id":network,"intelligence_snapshot":data["intelligence_snapshot"]});
            *live.write().unwrap() = data["exposure_snapshot"].clone();
            *risk_live.write().unwrap() = data["risk_snapshot"].clone();
            let config = json!({"contract":"bitcoin_semantics_config_v1","network_id":network,"detector_version":data["semantic_snapshot"]["detector_version"],"definition_hash":data["semantic_snapshot"]["definition_hash"]});
            *semantic_live.write().unwrap() = config.clone();
            let created = decoded(
                create(
                    app.clone(),
                    headers.clone(),
                    "bitcoin".into(),
                    subject.into(),
                    None,
                    format!("/api/bitcoin/wallet/{subject}/investigation?limit=40")
                        .parse()
                        .unwrap(),
                )
                .await
                .unwrap(),
            )
            .await;
            assert_eq!(created["source_validity"]["status"], "current");
            assert!(created["risk_engine"]["risk_score"].is_null());
            let current = json!({"network_id":network,"source_snapshot":data["source_snapshot"],"native_asset_snapshot":data["holdings"]["native_asset_snapshot"],"intelligence_snapshot":data["intelligence_snapshot"],"exposure_snapshot":data["exposure_snapshot"],"risk_snapshot":data["risk_snapshot"],"semantic_config":config});
            let mut tampered = created.clone();
            tampered["semantic_events"][0]["confidence"] = json!(1);
            annotate_source_validity(&mut tampered, Some(&current));
            assert_eq!(
                tampered["source_validity"]["status"], "unverified",
                "modified pattern evidence must break the snapshot binding"
            );
            let id = created["investigation"]["id"].as_str().unwrap().to_owned();
            let persisted: Value =
                serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
            let receipt = decoded(
                export(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            let reopened = decoded(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            for value in [&persisted, &created, &reopened] {
                for key in [
                    "semantic_events",
                    "semantic_coverage",
                    "semantic_snapshot",
                    "fingerprint",
                    "risk_snapshot",
                ] {
                    assert_eq!(value[key], data[key], "lost {name}/{key}");
                }
            }
            assert_eq!(receipt["investigation"]["state"], "saved");
            semantic_live.write().unwrap()["definition_hash"] = json!("0".repeat(64));
            let stale = decoded(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(stale["source_validity"]["status"], "stale");
            assert_eq!(stale["semantic_events"], data["semantic_events"]);
            assert_eq!(stale["risk_engine"]["risk_level"], "UNKNOWN");
            *semantic_live.write().unwrap() = config.clone();
            semantic_live.write().unwrap()["detector_version"] = json!("future_detector_version");
            let changed_version = decoded(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(changed_version["source_validity"]["status"], "stale");
            *semantic_live.write().unwrap() = Value::Null;
            let missing = decoded(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(missing["source_validity"]["status"], "unverified");
            *semantic_live.write().unwrap() = config;
            report[format!("semantic_{name}_created")] = created;
            report[format!("semantic_{name}_exported")] = reopened;
            report[format!("semantic_{name}_receipt")] = receipt;
            report[format!("semantic_{name}_stale")] = stale;
        }
    }
    if let Ok(file) = std::env::var("BITCOIN_HOLDINGS_ACCEPTANCE") {
        let fixture: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let data = &fixture["full"];
        let subject = data["address"].as_str().unwrap();
        *upstream.write().unwrap() = data.clone();
        let fresh_status = json!({"network_id":network,"source_snapshot":data["source_snapshot"],"native_asset_snapshot":data["holdings"]["native_asset_snapshot"]});
        *status.write().unwrap() = fresh_status.clone();
        *intel.write().unwrap() =
            json!({"network_id":network,"intelligence_snapshot":data["intelligence_snapshot"]});
        *live.write().unwrap() = data["exposure_snapshot"].clone();
        *risk_live.write().unwrap() = data["risk_snapshot"].clone();
        *semantic_live.write().unwrap() = json!({"contract":"bitcoin_semantics_config_v1","network_id":network,"detector_version":data["semantic_snapshot"]["detector_version"],"definition_hash":data["semantic_snapshot"]["definition_hash"]});
        let created = decoded(
            create(
                app.clone(),
                headers.clone(),
                "bitcoin".into(),
                subject.into(),
                None,
                format!("/api/bitcoin/wallet/{subject}/investigation?limit=40")
                    .parse()
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(created["source_validity"]["status"], "current");
        assert_eq!(created["holdings"]["balance_sats"], "299");
        let id = created["investigation"]["id"].as_str().unwrap().to_owned();
        let persisted: Value =
            serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
        let receipt = decoded(
            export(State(app.clone()), headers.clone(), Path(id.clone()))
                .await
                .unwrap(),
        )
        .await;
        let reopened = decoded(
            reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                .await
                .unwrap(),
        )
        .await;
        for value in [&created, &persisted, &reopened] {
            assert_eq!(
                value["holdings"], data["holdings"],
                "holdings/native metadata must survive create, Export and reopen"
            );
            assert_eq!(value["query_scope"], data["query_scope"]);
        }
        assert_eq!(receipt["investigation"]["state"], "saved");
        status.write().unwrap()["native_asset_snapshot"]["snapshot_id"] = json!("0".repeat(64));
        let stale = decoded(
            reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(stale["source_validity"]["status"], "stale");
        assert_eq!(stale["holdings"], data["holdings"]);
        assert_eq!(stale["risk_engine"]["risk_level"], "UNKNOWN");
        status.write().unwrap()["native_asset_snapshot"] = Value::Null;
        let missing = decoded(
            reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(missing["source_validity"]["status"], "unverified");
        *status.write().unwrap() = fresh_status;
        let mut bound = json!({"network_id":network,"source_snapshot":data["source_snapshot"],"native_asset_snapshot":data["holdings"]["native_asset_snapshot"],"intelligence_snapshot":data["intelligence_snapshot"],"exposure_snapshot":data["exposure_snapshot"],"risk_snapshot":data["risk_snapshot"],"semantic_config":semantic_live.read().unwrap().clone()});
        let mut changed = created.clone();
        changed["holdings"]["balance_sats"] = json!("0");
        annotate_source_validity(&mut changed, Some(&bound));
        assert_eq!(changed["source_validity"]["status"], "unverified");
        bound["source_snapshot"]["source_revision"] = json!("changed");
        let mut changed_source = created.clone();
        annotate_source_validity(&mut changed_source, Some(&bound));
        assert_eq!(changed_source["source_validity"]["status"], "stale");
        report["holdings_created"] = created;
        report["holdings_exported"] = reopened;
        report["holdings_receipt"] = receipt;
        report["holdings_stale"] = stale;
    }
    if let Ok(file) = std::env::var("BITCOIN_CLUSTER_ACCEPTANCE") {
        let mut proof: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let mut names = vec!["pending", "approved", "revoked", "change"];
        if let Ok(worker_file) = std::env::var("BITCOIN_WORKER_ACCEPTANCE") {
            let worker: Value =
                serde_json::from_slice(&std::fs::read(worker_file).unwrap()).unwrap();
            let data = worker["investigation"].clone();
            assert!(data["data_coverage"]["analytics_worker"].is_object());
            proof["worker_current"] = json!({"network_id":network,"cluster_snapshot":data["cluster_snapshot"],"cluster_config":{"contract":"bitcoin_clustering_config_v1","network_id":network,"detector_version":data["cluster_snapshot"]["detector_version"],"definition_hash":data["cluster_snapshot"]["definition_hash"]}});
            proof["worker"] = data;
            names.push("worker");
        }
        for name in names {
            let data = &proof[name];
            let address = data["address"].as_str().unwrap().to_owned();
            *upstream.write().unwrap() = data.clone();
            *live.write().unwrap() = data["exposure_snapshot"].clone();
            *risk_live.write().unwrap() = data["risk_snapshot"].clone();
            *status.write().unwrap() = json!({"network_id":network,"source_snapshot":data["source_snapshot"],"native_asset_snapshot":data["holdings"]["native_asset_snapshot"]});
            *intel.write().unwrap() =
                json!({"network_id":network,"intelligence_snapshot":data["intelligence_snapshot"]});
            *semantic_live.write().unwrap() = json!({"contract":"bitcoin_semantics_config_v1","network_id":network,"detector_version":data["semantic_snapshot"]["detector_version"],"definition_hash":data["semantic_snapshot"]["definition_hash"]});
            *cluster_override.write().unwrap() = None;
            let created = decoded(
                create(
                    app.clone(),
                    headers.clone(),
                    "bitcoin".into(),
                    address.clone(),
                    None,
                    format!("/api/bitcoin/wallet/{address}/investigation?limit=40")
                        .parse()
                        .unwrap(),
                )
                .await
                .unwrap(),
            )
            .await;
            assert_eq!(
                created["source_validity"]["status"], "current",
                "{name}: {created}"
            );
            assert!(created["risk_engine"]["risk_score"].is_null());
            let persisted: Value =
                serde_json::from_str(stored.read().unwrap()["payload"].as_str().unwrap()).unwrap();
            let id = created["investigation"]["id"].as_str().unwrap().to_owned();
            let receipt = decoded(
                export(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(receipt["investigation"]["state"], "saved");
            let reopened = decoded(
                reopen(State(app.clone()), headers.clone(), Path(id.clone()))
                    .await
                    .unwrap(),
            )
            .await;
            for result in [&created, &persisted, &reopened] {
                for key in [
                    "clusters",
                    "cluster_snapshot",
                    "cluster_coverage",
                    "data_coverage",
                ] {
                    assert_eq!(result[key], data[key], "lost {name} {key}");
                }
            }
            let mut stale = proof[format!("{name}_current")].clone();
            stale["cluster_snapshot"]["current"] = json!(false);
            *cluster_override.write().unwrap() = Some(stale);
            let historical = decoded(
                reopen(State(app.clone()), headers.clone(), Path(id))
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(historical["source_validity"]["status"], "stale");
            assert_eq!(historical["clusters"], data["clusters"]);
            assert!(historical["risk_engine"]["risk_score"].is_null());
            report[format!("cluster_{name}_created")] = created;
            report[format!("cluster_{name}_exported")] = reopened;
            report[format!("cluster_{name}_historical")] = historical;
        }
    }
    if let Ok(file) = std::env::var("BITCOIN_EXPOSURE_CENTRAL_ACCEPTANCE") {
        std::fs::write(file, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    server.abort();
}
