use super::{
    alerts, delivery, routes,
    tests::{operator, serve},
};
use crate::{App, Store, now};
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

async fn data(request: reqwest::RequestBuilder) -> anyhow::Result<Value> {
    Ok(request.send().await?.error_for_status()?.json().await?)
}

async fn result(
    store: &Store,
    watch: &Value,
    evidence: &Value,
    webhook: bool,
) -> anyhow::Result<()> {
    let lease = nanoid::nanoid!(32);
    let at = now();
    store
        .query(
            "MATCH (w:KytWatch {id:$id}) SET w.lease=$lease RETURN w.id",
            json!({"id":watch,"lease":lease}),
        )
        .await?;
    alerts::commit_check(store, watch, &lease, evidence, at, at + 900_000, webhook).await?;
    // A retry of the same completion must be a no-op after the lease was consumed.
    alerts::commit_check(store, watch, &lease, evidence, at, at + 900_000, webhook).await
}

#[tokio::test]
#[ignore = "requires a fresh disposable KYT_NEO4J_URL and KYT_NEO4J_PASSWORD"]
async fn alerts_survive_rechecks_and_webhook_retries_on_real_neo4j() -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let url = std::env::var("KYT_NEO4J_URL")?;
    let password = std::env::var("KYT_NEO4J_PASSWORD")?;
    let store = Store::new(http.clone(), &url, password.clone());
    store.initialize().await?;
    let app = App {
        store: store.clone(),
        http: http.clone(),
        upstreams: HashMap::new(),
        key: "fixture-key".into(),
        ttl_ms: 1_000,
        requests: Arc::new(Semaphore::new(2)),
    };
    let (api, api_task) = serve(routes().with_state(app)).await?;
    let alerts_url = format!("{api}/api/kyt/alerts");
    let watchlist = format!("{api}/api/kyt/watchlist");
    let mut owner = operator();
    owner.insert("x-aml-user", "fixture-alert-owner".parse()?);
    let mut assignee = operator();
    assignee.insert("x-aml-user", "fixture-alert-assignee".parse()?);
    let watch = data(http.post(&watchlist).headers(owner.clone()).json(&json!({
        "network":"ethereum","kind":"address","value":"0x0000000000000000000000000000000000000001",
        "system_id":"payments","customer_id":"opaque:01","threshold":25})))
    .await?;
    let watch_id = &watch["id"];
    let watch_url = format!("{watchlist}/{}", watch_id.as_str().unwrap());
    let matched = json!({"status":"matched","source_snapshot_id":"a".repeat(64),
        "assessment":{"matched":true,"score":69,"rule_score":69,"signals":[{"type":"fixture","contribution":69}]}});
    let watch_read = || data(http.get(&watch_url).headers(owner.clone()));
    let list = |url: &str, headers: &HeaderMap| data(http.get(url).headers(headers.clone()));
    let alert = |id: &Value, headers: &HeaderMap| {
        http.get(format!("{alerts_url}/{}", id.as_str().unwrap()))
            .headers(headers.clone())
    };
    let act = |id: &Value, headers: &HeaderMap, action: Value| {
        http.patch(format!("{alerts_url}/{}", id.as_str().unwrap()))
            .headers(headers.clone())
            .json(&action)
    };

    result(&store, watch_id, &matched, true).await?;
    let first = watch_read().await?["match_alert_id"].clone();
    for state in ["matched", "unknown", "not_indexed", "matched"] {
        let evidence = if state == "matched" {
            matched.clone()
        } else {
            json!({"status":state})
        };
        result(&store, watch_id, &evidence, true).await?;
        assert_eq!(watch_read().await?["match_alert_id"], first);
    }
    let listed = list(&alerts_url, &owner).await?;
    assert_eq!(listed["alerts"].as_array().unwrap().len(), 1);
    let created = &listed["alerts"][0];
    assert_eq!(created["status"], "open");
    assert_eq!(created["severity"], "high");
    assert_eq!(created["evidence"], matched);
    assert_eq!(created["delivery"]["status"], "pending");
    assert_eq!(
        alert(&first, &assignee).send().await?.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        act(&first, &owner, json!({"action":"resolve"}))
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    data(act(
        &first,
        &owner,
        json!({"action":"assign","assignee":"fixture-alert-assignee"}),
    ))
    .await?;
    assert_eq!(
        list(
            &format!("{alerts_url}?severity=high&status=open"),
            &assignee
        )
        .await?["alerts"][0]["id"],
        first
    );
    assert_eq!(
        act(&first, &assignee, json!({"action":"unassign"}))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    data(act(&first, &owner, json!({"action":"unassign"}))).await?;
    assert_eq!(
        alert(&first, &assignee).send().await?.status(),
        StatusCode::NOT_FOUND
    );
    data(act(
        &first,
        &owner,
        json!({"action":"assign","assignee":"fixture-alert-assignee"}),
    ))
    .await?;
    let acknowledged = data(act(&first, &assignee, json!({"action":"acknowledge"}))).await?;
    assert_eq!(acknowledged["status"], "acknowledged");
    let resolved = data(act(&first, &owner, json!({"action":"resolve"}))).await?;
    assert_eq!(resolved["status"], "resolved");
    assert_eq!(resolved["acknowledged_by"], "fixture-alert-assignee");
    assert_eq!(
        data(act(&first, &assignee, json!({"action":"resolve"}))).await?["resolved_by"],
        "fixture-alert-owner"
    );
    result(&store, watch_id, &matched, true).await?;
    assert_eq!(
        watch_read().await?["match_alert_id"],
        first,
        "a resolved alert must not be recreated while still matched"
    );

    let requests = Arc::new(Mutex::new(Vec::new()));
    let handler = post({
        let requests = requests.clone();
        move |headers: HeaderMap, Json(body): Json<Value>| {
            let requests = requests.clone();
            async move {
                if headers
                    .get("authorization")
                    .is_none_or(|h| h != "Bearer fixture-token")
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let key = headers["idempotency-key"].to_str().unwrap().to_owned();
                let mut requests = requests.lock().unwrap();
                requests.push((key, body));
                match requests.len() {
                    1 => (StatusCode::TOO_MANY_REQUESTS, [("Retry-After", "120")]).into_response(),
                    2 => StatusCode::NO_CONTENT.into_response(),
                    _ => StatusCode::BAD_REQUEST.into_response(),
                }
            }
        }
    });
    let (destination, webhook_task) = serve(Router::new().route("/events", handler)).await?;
    let webhook_url = format!("{destination}/events").parse::<reqwest::Url>()?;
    let webhook = delivery::Webhook::fixture(webhook_url.clone())?;
    let before = now();
    delivery::tick(&store, &webhook).await?;
    let retry = data(alert(&first, &owner)).await?;
    assert_eq!(retry["delivery"]["status"], "retrying");
    assert_eq!(retry["delivery"]["attempts"], 1);
    assert_eq!(retry["delivery"]["last_http_status"], 429);
    assert!(retry["delivery"]["next_attempt_at_ms"].as_i64().unwrap() >= before + 120_000);
    assert_eq!(
        list(&format!("{alerts_url}/backlog"), &owner).await?["delivery"][0]["status"],
        "retrying"
    );
    delivery::tick(&store, &webhook).await?;
    assert_eq!(requests.lock().unwrap().len(), 1);
    store
        .query(
            "MATCH (a:KytAlert {id:$id}) SET a.delivery_due_at_ms=0 RETURN a.id",
            json!({"id":first}),
        )
        .await?;
    let reopened = Store::new(http.clone(), &url, password);
    delivery::tick(&reopened, &delivery::Webhook::fixture(webhook_url)?).await?;
    let delivered = data(alert(&first, &owner)).await?;
    assert_eq!(delivered["delivery"]["status"], "delivered");
    assert_eq!(delivered["delivery"]["attempts"], 2);
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0], requests[1]);
        assert_eq!(requests[0].0, first.as_str().unwrap());
        assert_eq!(requests[0].1["alert"]["evidence"], matched);
    }

    let clear = json!({"status":"no_match"});
    result(&store, watch_id, &clear, true).await?;
    assert!(watch_read().await?["match_alert_id"].is_null());
    result(&store, watch_id, &matched, true).await?;
    let second = watch_read().await?["match_alert_id"].clone();
    assert_ne!(second, first);
    delivery::tick(&store, &webhook).await?;
    let rejected = data(alert(&second, &owner)).await?;
    assert_eq!(rejected["delivery"]["status"], "failed");
    assert_eq!(rejected["delivery"]["last_http_status"], 400);
    for (patch, error) in [
        (
            "a.delivery_status='sending',a.delivery_attempts=6,a.delivery_due_at_ms=0,a.delivery_lease='abandoned'",
            "attempt_limit",
        ),
        ("a.delivery_expires_at_ms=0", "queue_expired"),
    ] {
        result(&store, watch_id, &clear, true).await?;
        result(&store, watch_id, &matched, true).await?;
        let id = watch_read().await?["match_alert_id"].clone();
        store
            .query(
                &format!("MATCH (a:KytAlert {{id:$id}}) SET {patch} RETURN a.id"),
                json!({"id":id}),
            )
            .await?;
        delivery::tick(&store, &webhook).await?;
        let failed = data(alert(&id, &owner)).await?;
        assert_eq!(failed["delivery"]["status"], "failed");
        assert_eq!(failed["delivery"]["last_error"], error);
    }
    assert_eq!(requests.lock().unwrap().len(), 3);
    http.delete(&watch_url)
        .headers(owner.clone())
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(data(alert(&first, &owner)).await?["status"], "resolved");
    api_task.abort();
    webhook_task.abort();
    Ok(())
}
