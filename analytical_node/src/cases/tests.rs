use super::*;
use crate::user_owner_hash;
use aml_chain_api::{ApiPath, Network, kyt};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

async fn serve(router: Router) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Ok((url, task))
}

fn headers(actor: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-aml-user", actor.parse().unwrap());
    headers.insert("x-aml-service-key", "fixture-key".parse().unwrap());
    headers
}

async fn data(request: reqwest::RequestBuilder) -> anyhow::Result<Value> {
    Ok(request.send().await?.error_for_status()?.json().await?)
}

#[tokio::test]
#[ignore = "requires fresh disposable CASE_NEO4J_URL and CASE_NEO4J_PASSWORD"]
async fn case_preserves_evidence_and_review_timeline_on_real_neo4j() -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let store = Store::new(
        http.clone(),
        &std::env::var("CASE_NEO4J_URL")?,
        std::env::var("CASE_NEO4J_PASSWORD")?,
    );
    store.initialize().await?;
    let mut receipts = HashMap::new();
    let mut hashes = Vec::new();
    for (network, digit) in Network::ALL.into_iter().zip(['a', 'b', 'c', 'd']) {
        let hash = kyt::transaction_hash(network, &digit.to_string().repeat(64))?;
        let block = kyt::transaction_hash(network, &"9".repeat(64))?;
        receipts.insert(hash.clone(),json!({"network_id":network.id(),"tx_hash":hash,"ambiguous":false,
            "source_snapshot":{"contract":"canonical_v1","network_id":network.id(),"snapshot_id":"1".repeat(64),
                "initialized":true,"as_of_block":10},
            "occurrence":{"block_number":7,"block_hash":block,"participants":[],"participants_truncated":false}}));
        hashes.push((network, hash));
    }
    let missing_hash = kyt::transaction_hash(Network::Ethereum, &"f".repeat(64))?;
    receipts.insert(
        missing_hash.clone(),
        json!({"network_id":Network::Ethereum.id(),
        "tx_hash":missing_hash,"ambiguous":false,"source_snapshot":{"contract":"canonical_v1",
            "network_id":Network::Ethereum.id(),"snapshot_id":"1".repeat(64),
            "initialized":true,"as_of_block":10},"occurrence":null}),
    );
    let offline = Arc::new(AtomicBool::new(false));
    let adapter = Router::new().route(
        kyt::TRANSACTION_ROUTE,
        get({
            let offline = offline.clone();
            let receipts = Arc::new(receipts);
            move |ApiPath(hash): ApiPath<String>| {
                let value = receipts.get(&hash).cloned();
                let offline = offline.load(Ordering::SeqCst);
                async move {
                    if offline {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    let Some(value) = value else {
                        return StatusCode::NOT_FOUND.into_response();
                    };
                    Json(value).into_response()
                }
            }
        }),
    );
    let (adapter_url, adapter_task) = serve(adapter).await?;
    let app = App {
        store: store.clone(),
        http: http.clone(),
        key: "fixture-key".into(),
        ttl_ms: 1_000,
        requests: Arc::new(tokio::sync::Semaphore::new(2)),
        upstreams: Network::ALL
            .into_iter()
            .map(|chain| (chain.name().into(), adapter_url.clone()))
            .collect(),
    };
    let (base, api_task) = serve(routes().with_state(app)).await?;
    let cases = format!("{base}/api/cases");
    let owner = headers("case-owner");
    let reviewer = headers("case-reviewer");
    let outsider = headers("case-outsider");
    let scope = user_owner_hash("case-owner");
    let key = nanoid::nanoid!(20);
    let create = |title: &str| {
        http.post(&cases)
            .headers(owner.clone())
            .header("Idempotency-Key", &key)
            .json(&json!({"title":title}))
    };
    let created = data(create("Wallet review")).await?;
    assert_eq!(data(create("Wallet review")).await?, created);
    assert_eq!(
        create("Different case").send().await?.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(created["status"], "draft");
    let id = created["id"].as_str().unwrap();
    let case_url = format!("{cases}/{id}");
    let event_url = format!("{case_url}/events");
    let report_url = format!("{case_url}/report");
    let event = |actor: &HeaderMap, key: &str, body: Value| {
        http.post(&event_url)
            .headers(actor.clone())
            .header("Idempotency-Key", key)
            .json(&body)
    };
    let read = |actor: &HeaderMap| http.get(&case_url).headers(actor.clone());
    assert_eq!(
        read(&outsider).send().await?.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        http.get(&report_url)
            .headers(outsider.clone())
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        http.get(&case_url)
            .header("x-aml-service-key", "fixture-key")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        event(&owner, "submit-early", json!({"action":"submit_review"}))
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );

    let investigation = nanoid::nanoid!(32);
    let snapshot = json!({"source_snapshot":{"snapshot_id":"2".repeat(64)},
        "source_validity":{"status":"current"},"risk_engine":{"risk_score":70,"risk_level":"HIGH","policy_version":"fixture"},
        "graph":{"nodes":[],"edges":[]}});
    let payload = snapshot.to_string();
    store.create(json!({"metadata":{"id":investigation,"state":"temporary","network_id":"eip155:1","mode":"wallet",
        "expires_at_unix_ms":now()+60_000,"snapshot_hash":format!("{:x}",Sha256::digest(payload.as_bytes()))},
        "owner":scope,"payload":payload,"nodes":[],"edges":[]})).await?;
    let attach = json!({"action":"attach_investigation","investigation_id":investigation});
    assert_eq!(
        event(&owner, "attach-graph", attach.clone())
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert!(store.export(&investigation, &scope, now()).await?.is_some());
    let graph = data(event(&owner, "attach-graph", attach.clone())).await?;
    assert_eq!(
        data(event(&owner, "attach-graph", attach.clone())).await?,
        graph
    );
    assert_eq!(
        event(&owner, "attach-again", attach).send().await?.status(),
        StatusCode::CONFLICT
    );

    let alert_id = aml_risk_policy::digest(&json!(["fixture-alert", id]));
    let other_alert = aml_risk_policy::digest(&json!(["other-alert", id]));
    store.query(r#"
        CREATE (a:KytAlert {id:$id,owner:$owner,network_id:'eip155:1',severity:'high',evidence_json:$evidence})
        CREATE (b:KytAlert {id:$other,owner:'other',assignee_hash:$owner,evidence_json:$evidence})
        RETURN a.id,b.id
    "#,json!({"id":alert_id,"other":other_alert,"owner":scope,"evidence":json!({"status":"matched"}).to_string()})).await?;
    assert_eq!(
        event(
            &owner,
            "foreign-alert",
            json!({"action":"attach_alert","alert_id":other_alert})
        )
        .send()
        .await?
        .status(),
        StatusCode::NOT_FOUND
    );
    let alert = data(event(
        &owner,
        "attach-alert",
        json!({"action":"attach_alert","alert_id":alert_id}),
    ))
    .await?;
    assert_eq!(
        alert["details"]["evidence_at_detection"]["status"],
        "matched"
    );

    assert_eq!(
        event(
            &owner,
            "missing-tx",
            json!({"action":"attach_transaction","network":"ethereum","hash":missing_hash})
        )
        .send()
        .await?
        .status(),
        StatusCode::CONFLICT
    );
    for (network, hash) in hashes {
        let key = format!("attach-{}", network.name());
        let body = json!({"action":"attach_transaction","network":network.name(),"hash":hash});
        let attached = data(event(&owner, &key, body.clone())).await?;
        assert_eq!(
            attached["details"]["transaction"]["occurrence"]["block_number"],
            7
        );
        assert_eq!(
            attached["details"]["transaction"]["network_id"],
            network.id()
        );
        offline.store(true, Ordering::SeqCst);
        assert_eq!(data(event(&owner, &key, body)).await?, attached);
        offline.store(false, Ordering::SeqCst);
    }

    let note = json!({"action":"add_note","text":"شاهد اولیه برای بازبینی\nAnalyst note"});
    let (a, b) = tokio::join!(
        data(event(&owner, "parallel-note", note.clone())),
        data(event(&owner, "parallel-note", note.clone()))
    );
    assert_eq!(a?, b?);
    assert_eq!(
        event(
            &owner,
            "parallel-note",
            json!({"action":"add_note","text":"Different note"})
        )
        .send()
        .await?
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        event(
            &owner,
            "review-self",
            json!({"action":"assign_reviewer","reviewer":"case-owner"})
        )
        .send()
        .await?
        .status(),
        StatusCode::BAD_REQUEST
    );
    data(event(
        &owner,
        "assign-reviewer",
        json!({"action":"assign_reviewer","reviewer":"case-reviewer"}),
    ))
    .await?;
    assert!(
        data(http.get(&cases).headers(reviewer.clone())).await?["cases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == id)
    );
    let evidence_url = format!("{case_url}/evidence/{}", graph["id"].as_str().unwrap());
    assert_eq!(
        data(http.get(&evidence_url).headers(reviewer.clone())).await?["historical_investigation"],
        snapshot
    );
    store
        .query(
            "MATCH (i:Investigation {id:$id}) SET i.payload_json='{}' RETURN i.id",
            json!({"id":investigation}),
        )
        .await?;
    assert_eq!(
        http.get(&evidence_url)
            .headers(reviewer.clone())
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    store
        .query(
            "MATCH (i:Investigation {id:$id}) SET i.payload_json=$payload RETURN i.id",
            json!({"id":investigation,"payload":payload}),
        )
        .await?;
    assert_eq!(
        http.get(&evidence_url)
            .headers(outsider)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        store
            .read(&investigation, &user_owner_hash("case-reviewer"), now())
            .await?
            .is_none()
    );
    data(event(
        &owner,
        "submit-first",
        json!({"action":"submit_review"}),
    ))
    .await?;
    assert_eq!(
        event(&owner, "owner-approve", json!({"action":"approve"}))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        event(&owner, "locked-note", note.clone())
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    data(event(
        &reviewer,
        "review-changes",
        json!({"action":"request_changes","reason":"Document source"}),
    ))
    .await?;
    assert_eq!(
        data(read(&owner)).await?["case"]["status"],
        "changes_requested"
    );
    data(event(&owner, "correct-note", note)).await?;
    data(event(
        &owner,
        "submit-final",
        json!({"action":"submit_review"}),
    ))
    .await?;
    let approved = data(event(
        &reviewer,
        "review-approve",
        json!({"action":"approve","comment":"Reviewed"}),
    ))
    .await?;
    assert_eq!(
        data(event(
            &reviewer,
            "review-approve",
            json!({"action":"approve","comment":"Reviewed"})
        ))
        .await?,
        approved
    );
    let current = data(read(&owner)).await?;
    assert_eq!(current["case"]["status"], "closed");
    assert_eq!(current["case"]["evidence_count"], 6);
    let timeline = current["timeline"].as_array().unwrap();
    for (index, event) in timeline.iter().enumerate() {
        assert_eq!(event["sequence"], index + 1);
    }
    assert_eq!(timeline.last().unwrap()["kind"], "review_approved");
    let tail = data(
        http.get(format!("{case_url}?after={}", timeline.len() - 1))
            .headers(owner.clone()),
    )
    .await?;
    assert_eq!(tail["timeline"].as_array().unwrap().len(), 1);
    assert_eq!(
        event(
            &owner,
            "closed-note",
            json!({"action":"add_note","text":"Late note"})
        )
        .send()
        .await?
        .status(),
        StatusCode::CONFLICT
    );
    let report = http
        .get(report_url)
        .headers(reviewer)
        .send()
        .await?
        .error_for_status()?;
    assert!(
        report.headers()["content-disposition"]
            .to_str()?
            .contains("attachment")
    );
    assert_eq!(report.headers()["cache-control"], "no-store");
    let content = report.text().await?;
    assert!(
        content.contains("Wallet review")
            && content.contains("شاهد اولیه")
            && content.contains("review_approved")
            && content.contains(&alert_id)
    );
    api_task.abort();
    adapter_task.abort();
    Ok(())
}
