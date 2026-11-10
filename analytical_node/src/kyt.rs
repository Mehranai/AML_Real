#[cfg(test)]
mod alert_tests;
mod alerts;
mod delivery;
mod screening;
pub(crate) use screening::read_transaction;
#[cfg(test)]
mod tests;

use super::{App, Failure, Owner, authenticated_operator, now, response, valid_address};
use aml_chain_api::{ApiPath as Path, ApiQuery as Query, Network, kyt};
use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::str::FromStr;
use std::time::Duration;

const PAGE_SIZE: usize = 50;
const INTERVAL_MS: i64 = 900_000;
const RETRY_MS: i64 = 60_000;
const LEASE_MS: i64 = 90_000;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Address,
    Transaction,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Rule {
    #[default]
    EvidenceScore,
    ApprovedLabel,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    network: String,
    kind: Kind,
    value: String,
    system_id: String,
    customer_id: String,
    #[serde(default)]
    rule: Rule,
    threshold: Option<u8>,
}

#[derive(Deserialize)]
struct Target {
    network_id: String,
    kind: Kind,
    value: String,
    rule: Rule,
    threshold: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<String>,
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/kyt/watchlist", get(list).post(register))
        .route("/api/kyt/watchlist/{id}", get(read).delete(stop))
        .merge(alerts::routes())
        .layer(DefaultBodyLimit::max(4096))
}

fn watch_owner(app: &App, headers: &HeaderMap) -> Result<Owner, Failure> {
    authenticated_operator(app, headers).map(|(owner, _)| owner)
}

fn watch_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn reference(value: &str) -> bool {
    (1..=96).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

fn registration(input: Input, owner: &str) -> Result<Value, Failure> {
    let network = Network::ALL
        .into_iter()
        .find(|chain| chain.name() == input.network)
        .ok_or(Failure(StatusCode::BAD_REQUEST, "Unsupported network"))?;
    if !reference(&input.system_id) || !reference(&input.customer_id) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "Invalid system or customer identifier",
        ));
    }
    let value = match input.kind {
        Kind::Address if network == Network::Bitcoin => bitcoin::Address::from_str(&input.value)
            .map_err(anyhow::Error::from)
            .and_then(|address| {
                Ok(address
                    .require_network(bitcoin::Network::Bitcoin)?
                    .to_string())
            }),
        Kind::Address => kyt::wallet_address(network, &input.value),
        Kind::Transaction => kyt::transaction_hash(network, &input.value),
    }
    .map_err(|_| Failure(StatusCode::BAD_REQUEST, "Invalid watch target"))?;
    if matches!(input.kind, Kind::Address) && !valid_address(network.name(), &value) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "Unsupported wallet address",
        ));
    }
    let threshold = input.threshold.unwrap_or(25);
    let maximum = match input.rule {
        Rule::EvidenceScore => 100.0,
        Rule::ApprovedLabel => aml_risk_policy::LABEL_WEIGHT,
    };
    if threshold == 0 || f64::from(threshold) > maximum {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "Threshold outside the rule's score range",
        ));
    }
    let id = aml_risk_policy::digest(&json!([
        owner,
        network.id(),
        input.kind,
        value,
        input.system_id,
        input.customer_id
    ]));
    Ok(
        json!({"id":id,"owner":owner,"network":network.name(),"network_id":network.id(),
        "kind":input.kind,"value":value,"system_id":input.system_id,"customer_id":input.customer_id,
        "rule":input.rule,"threshold":threshold,"now":now()}),
    )
}

fn entry(mut item: Value) -> anyhow::Result<Value> {
    let result = item
        .as_object_mut()
        .context("invalid watch record")?
        .remove("result_json");
    item["result"] = match result.as_ref().and_then(Value::as_str) {
        Some(serialized) => serde_json::from_str(serialized)?,
        None => Value::Null,
    };
    Ok(item)
}

fn unavailable(error: impl std::fmt::Display) -> Failure {
    eprintln!("KYT storage unavailable: {error}");
    Failure(
        StatusCode::SERVICE_UNAVAILABLE,
        "KYT watchlist storage is unavailable",
    )
}

const FIELDS: &str =
    "w{.id,.network,.network_id,.kind,.value,.system_id,.customer_id,.rule,.threshold,
    .created_at_ms,.updated_at_ms,.last_checked_at_ms,.next_check_at_ms,.result_json,.match_alert_id}";

async fn register(
    State(app): State<App>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<Response, Failure> {
    let owner = watch_owner(&app, &headers)?;
    let params = registration(input, &owner.hash)?;
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MERGE (w:KytWatch {{id:$id}})
        ON CREATE SET w.owner=$owner,w.network=$network,w.network_id=$network_id,w.kind=$kind,
            w.value=$value,w.system_id=$system_id,w.customer_id=$customer_id,
            w.created_at_ms=$now,w.active=false,w.lock_version=0
        SET w.lock_version=w.lock_version+1
        WITH w, (NOT w.active OR w.threshold <> $threshold OR w.rule <> $rule) AS changed
        FOREACH (_ IN CASE WHEN changed THEN [1] ELSE [] END |
            SET w.active=true,w.threshold=$threshold,w.rule=$rule,w.next_check_at_ms=$now,
                w.updated_at_ms=$now,w.result_json=null,w.last_checked_at_ms=null,w.lease=null,
                w.match_alert_id=null)
        RETURN {FIELDS}"#
            ),
            params,
        )
        .await
        .map_err(unavailable)?;
    let item = rows
        .into_iter()
        .next()
        .context("watch registration returned no record")
        .map_err(unavailable)?;
    Ok(response(
        entry(item[0].clone()).map_err(unavailable)?,
        owner,
    ))
}

async fn list(
    State(app): State<App>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Response, Failure> {
    let owner = watch_owner(&app, &headers)?;
    if page.after.as_deref().is_some_and(|id| !watch_id(id)) {
        return Err(Failure(StatusCode::BAD_REQUEST, "Invalid watchlist cursor"));
    }
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MATCH (w:KytWatch {{owner:$owner,active:true}})
        WHERE w.id > $after
        RETURN {FIELDS} ORDER BY w.id LIMIT $limit"#
            ),
            json!({"owner":owner.hash,"after":page.after.unwrap_or_default(),"limit":PAGE_SIZE+1}),
        )
        .await
        .map_err(unavailable)?;
    let more = rows.len() > PAGE_SIZE;
    let entries: Vec<_> = rows
        .into_iter()
        .take(PAGE_SIZE)
        .map(|row| entry(row[0].clone()))
        .collect::<anyhow::Result<_>>()
        .map_err(unavailable)?;
    let next = more.then(|| &entries[PAGE_SIZE - 1]["id"]);
    Ok(response(
        json!({"entries":entries,"next_after":next}),
        owner,
    ))
}

fn missing() -> Failure {
    Failure(StatusCode::NOT_FOUND, "Active watch entry not found")
}

async fn read(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let owner = watch_owner(&app, &headers)?;
    if !watch_id(&id) {
        return Err(missing());
    }
    let rows = app
        .store
        .query(
            &format!("MATCH (w:KytWatch {{id:$id,owner:$owner,active:true}}) RETURN {FIELDS}"),
            json!({"id":id,"owner":owner.hash}),
        )
        .await
        .map_err(unavailable)?;
    let item = rows.into_iter().next().ok_or_else(missing)?;
    Ok(response(
        entry(item[0].clone()).map_err(unavailable)?,
        owner,
    ))
}

async fn stop(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let owner = watch_owner(&app, &headers)?;
    if !watch_id(&id) {
        return Err(missing());
    }
    let rows = app
        .store
        .query(
            r#"
        MATCH (w:KytWatch {id:$id,owner:$owner})
        SET w.lock_version=w.lock_version+1
        SET w.active=false,w.lease=null,w.next_check_at_ms=null
        RETURN w.id"#,
            json!({"id":id,"owner":owner.hash}),
        )
        .await
        .map_err(unavailable)?;
    if rows.is_empty() {
        return Err(missing());
    }
    Ok(response(json!({"id":id,"active":false}), owner))
}

pub fn start(app: App) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let webhook = delivery::Webhook::from_env()?;
    let webhook_enabled = webhook.is_some();
    Ok(tokio::spawn(async move {
        tokio::join!(
            run(&app, webhook_enabled),
            delivery::run(&app.store, webhook)
        );
    }))
}

async fn run(app: &App, webhook_enabled: bool) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if let Err(error) = check_due(app, webhook_enabled).await {
            eprintln!("KYT check failed: {error:#}");
        }
    }
}

async fn check_due(app: &App, webhook_enabled: bool) -> anyhow::Result<()> {
    let Ok(_permit) = app.requests.try_acquire() else {
        return Ok(());
    };
    let claim = nanoid::nanoid!(32);
    let started = now();
    let rows = app
        .store
        .query(
            r#"
        MATCH (w:KytWatch {active:true})
        WHERE w.next_check_at_ms <= $now
        WITH w ORDER BY w.next_check_at_ms,w.id LIMIT 1
        SET w.lock_version=w.lock_version+1
        WITH w WHERE w.active AND w.next_check_at_ms <= $now
        SET w.lease=$lease,w.next_check_at_ms=$expires
        RETURN w.id,w{.network_id,.kind,.value,.rule,.threshold}"#,
            json!({"now":started,"lease":claim,"expires":started+LEASE_MS}),
        )
        .await?;
    let Some(job) = rows.first() else {
        return Ok(());
    };
    let target: Target = serde_json::from_value(job[1].clone())?;
    let (result, delay) =
        match tokio::time::timeout(Duration::from_secs(45), screening::check(app, &target)).await {
            Ok(Ok(result)) => (result, INTERVAL_MS),
            _ => (
                json!({"status":"unknown","reason":"source_unavailable"}),
                RETRY_MS,
            ),
        };
    let checked = now();
    alerts::commit_check(
        &app.store,
        &job[0],
        &claim,
        &result,
        checked,
        checked + delay,
        webhook_enabled,
    )
    .await?;
    Ok(())
}
