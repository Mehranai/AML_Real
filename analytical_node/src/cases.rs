mod events;
mod evidence;
#[cfg(test)]
mod tests;

use crate::{App, Failure, Owner, Store, authenticated_operator, now, response};
use aml_chain_api::{ApiPath as Path, ApiQuery as Query};
use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write;

const PAGE: usize = 50;
const REPORT_LIMIT: usize = 250;
const FIELDS: &str = "c{.id,.title,.status,.reviewer,.created_at_ms,.updated_at_ms,.closed_at_ms,.revision,.evidence_count}";
const EVENT_FIELDS: &str = "e{.id,.sequence,.kind,.actor,.at_ms,.details_json}";

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/cases", get(list).post(create))
        .route("/api/cases/{id}", get(read))
        .route(
            "/api/cases/{id}/events",
            axum::routing::post(events::append),
        )
        .route("/api/cases/{id}/evidence/{event_id}", get(evidence::read))
        .route("/api/cases/{id}/report", get(report))
        .layer(DefaultBodyLimit::max(8192))
}

fn unavailable(error: impl std::fmt::Display) -> Failure {
    eprintln!("Case storage unavailable: {error}");
    Failure(
        StatusCode::SERVICE_UNAVAILABLE,
        "Case storage is unavailable",
    )
}

fn missing() -> Failure {
    Failure(StatusCode::NOT_FOUND, "Case or evidence not found")
}

fn conflict(message: &'static str) -> Failure {
    Failure(StatusCode::CONFLICT, message)
}

fn valid_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn require_id(value: &str) -> Result<(), Failure> {
    if !valid_id(value) {
        return Err(missing());
    }
    Ok(())
}

fn request_key(headers: &HeaderMap) -> Result<&str, Failure> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|key| {
            (8..=96).contains(&key.len())
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or(Failure(
            StatusCode::BAD_REQUEST,
            "A valid Idempotency-Key is required",
        ))
}

fn text(value: &str, max: usize, multiline: bool) -> Result<&str, Failure> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
    {
        return Err(Failure(StatusCode::BAD_REQUEST, "Invalid case text"));
    }
    Ok(value)
}

fn decode_event(mut event: Value) -> anyhow::Result<Value> {
    let details = event
        .as_object_mut()
        .context("invalid case event")?
        .remove("details_json")
        .context("missing case event details")?;
    event["details"] = serde_json::from_str(details.as_str().context("invalid case details")?)?;
    Ok(event)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    title: String,
}

async fn create(
    State(app): State<App>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Result<Response, Failure> {
    let (operator, actor) = authenticated_operator(&app, &headers)?;
    let key = request_key(&headers)?;
    let title = text(&input.title, 240, false)?;
    let id = aml_risk_policy::digest(&json!(["case_v1", operator.hash, key]));
    let event_id = aml_risk_policy::digest(&json!(["case_created_v1", id]));
    let request_hash = aml_risk_policy::digest(&json!({"title":title}));
    let rows = app.store.case_mutation(&format!(r#"
        MERGE (c:InvestigationCase {{id:$id}})
        ON CREATE SET c.owner=$owner,c.title=$title,c.status='draft',c.created_at_ms=$now,
            c.updated_at_ms=$now,c.revision=1,c.evidence_count=0,c.lock_version=0,c.create_hash=$request_hash
        WITH c WHERE c.owner=$owner AND c.create_hash=$request_hash
        MERGE (e:CaseEvent {{id:$event_id}})
        ON CREATE SET e.case_id=c.id,e.sequence=1,e.kind='created',e.actor=$actor,
            e.at_ms=$now,e.details_json=$details
        MERGE (c)-[:HAS_EVENT]->(e)
        RETURN {FIELDS}"#),
        json!({"id":id,"event_id":event_id,"owner":operator.hash,"actor":actor,"title":title,
            "now":now(),"request_hash":request_hash,"details":json!({"title":title}).to_string()}))
        .await.map_err(unavailable)?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| conflict("Idempotency-Key was used with different case data"))?;
    Ok(response(row[0].clone(), operator))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListPage {
    after: Option<String>,
}

async fn list(
    State(app): State<App>,
    headers: HeaderMap,
    Query(page): Query<ListPage>,
) -> Result<Response, Failure> {
    let (operator, _) = authenticated_operator(&app, &headers)?;
    if page.after.as_deref().is_some_and(|id| !valid_id(id)) {
        return Err(Failure(StatusCode::BAD_REQUEST, "Invalid case cursor"));
    }
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MATCH (c:InvestigationCase)
        WHERE (c.owner=$operator OR c.reviewer_hash=$operator) AND c.id>$after
        RETURN {FIELDS} ORDER BY c.id LIMIT $limit"#
            ),
            json!({"operator":operator.hash,"after":page.after.unwrap_or_default(),"limit":PAGE+1}),
        )
        .await
        .map_err(unavailable)?;
    let more = rows.len() > PAGE;
    let cases: Vec<_> = rows
        .into_iter()
        .take(PAGE)
        .map(|row| row[0].clone())
        .collect();
    let next = more.then(|| &cases[PAGE - 1]["id"]);
    Ok(response(json!({"cases":cases,"next_after":next}), operator))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimelinePage {
    after: Option<i64>,
}

async fn view(
    store: &Store,
    operator: &Owner,
    id: &str,
    after: i64,
    latest: bool,
) -> Result<(Value, Vec<Value>), Failure> {
    let order = if latest { "DESC" } else { "ASC" };
    let limit = if latest { REPORT_LIMIT } else { PAGE + 1 };
    let rows = store
        .query(
            &format!(
                r#"
        MATCH (c:InvestigationCase {{id:$id}})
        WHERE c.owner=$operator OR c.reviewer_hash=$operator
        WITH c,{FIELDS} AS record
        CALL {{
            WITH c,record
            MATCH (c)-[:HAS_EVENT]->(e:CaseEvent)
            WHERE e.sequence>$after AND e.sequence<=record.revision
            WITH e ORDER BY e.sequence {order} LIMIT $limit
            RETURN collect({EVENT_FIELDS}) AS events
        }}
        RETURN record,events"#
            ),
            json!({"id":id,"operator":operator.hash,"after":after,"limit":limit}),
        )
        .await
        .map_err(unavailable)?;
    let row = rows.into_iter().next().ok_or_else(missing)?;
    let events = row[1]
        .as_array()
        .context("invalid timeline")
        .map_err(unavailable)?
        .iter()
        .cloned()
        .map(decode_event)
        .collect::<anyhow::Result<_>>()
        .map_err(unavailable)?;
    Ok((row[0].clone(), events))
}

async fn read(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(page): Query<TimelinePage>,
) -> Result<Response, Failure> {
    let (operator, _) = authenticated_operator(&app, &headers)?;
    require_id(&id)?;
    let after = page.after.unwrap_or(0);
    if after < 0 {
        return Err(Failure(StatusCode::BAD_REQUEST, "Invalid timeline cursor"));
    }
    let (case, mut events) = view(&app.store, &operator, &id, after, false).await?;
    let next = (events.len() > PAGE).then(|| events[PAGE - 1]["sequence"].clone());
    events.truncate(PAGE);
    Ok(response(
        json!({"case":case,"timeline":events,"next_after":next}),
        operator,
    ))
}

async fn report(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let (operator, _) = authenticated_operator(&app, &headers)?;
    require_id(&id)?;
    let (case, mut events) = view(&app.store, &operator, &id, 0, true).await?;
    let mut body = String::new();
    writeln!(
        body,
        "AML case report\nID {}\nTitle {}\nState {}\nReviewer {}\nRevision {}\nEvidence count {}",
        id,
        crate::risk::text(&case["title"]),
        crate::risk::text(&case["status"]),
        crate::risk::text(&case["reviewer"]),
        case["revision"],
        case["evidence_count"]
    )
    .unwrap();
    let first = events.last().map(|event| &event["sequence"]);
    if first.is_some_and(|sequence| sequence != 1) {
        writeln!(
            body,
            "Timeline includes the latest {REPORT_LIMIT} events. Earlier events remain in the case."
        )
        .unwrap();
    }
    writeln!(body, "\nTimeline (Unix milliseconds)\n").unwrap();
    events.reverse();
    for event in events {
        writeln!(
            body,
            "{}  {}  {}  {}\n{}\n",
            event["sequence"],
            event["at_ms"],
            crate::risk::text(&event["actor"]),
            crate::risk::text(&event["kind"]),
            event["details"]
        )
        .unwrap();
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".into()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"aml-case-{id}.txt\""),
            ),
            (header::CACHE_CONTROL, "no-store".into()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
        ],
        body,
    )
        .into_response())
}
