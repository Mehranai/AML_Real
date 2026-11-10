use super::{PAGE_SIZE, unavailable, watch_id, watch_owner};
use crate::{App, Failure, Store, now, response, user_owner_hash};
use aml_chain_api::{ApiPath as Path, ApiQuery as Query};
use anyhow::Context;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) const DELIVERY_AGE_MS: i64 = 86_400_000;
const FIELDS: &str = "a{.id,.watch_id,.network_id,.kind,.value,.system_id,.customer_id,
    .rule,.threshold,.severity,.status,.created_at_ms,.evidence_json,.assignee,.assigned_at_ms,
    .assigned_by,.acknowledged_at_ms,.acknowledged_by,.resolved_at_ms,.resolved_by,
    delivery:a{status:a.delivery_status,attempts:a.delivery_attempts,next_attempt_at_ms:
    CASE WHEN a.delivery_status IN ['pending','retrying'] THEN a.delivery_due_at_ms ELSE null END,
    last_attempt_at_ms:a.delivery_last_attempt_at_ms,last_http_status:a.delivery_last_http_status,
    last_error:a.delivery_last_error,.delivered_at_ms}}";

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/kyt/alerts", get(list))
        .route("/api/kyt/alerts/backlog", get(backlog))
        .route("/api/kyt/alerts/{id}", get(read).patch(update))
}

fn severity(result: &Value) -> Option<&'static str> {
    if result["status"] != "matched" {
        return None;
    }
    let score = result["assessments"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(result.get("assessment"))
        .filter(|assessment| assessment["matched"] == true)
        .filter_map(|assessment| assessment["score"].as_f64())
        .reduce(f64::max)?;
    Some(if score >= aml_risk_policy::HIGH {
        "high"
    } else if score >= aml_risk_policy::MEDIUM {
        "medium"
    } else {
        "low"
    })
}

pub(super) async fn commit_check(
    store: &Store,
    watch_id: &Value,
    lease: &str,
    result: &Value,
    checked: i64,
    next: i64,
    webhook_enabled: bool,
) -> anyhow::Result<()> {
    let severity = severity(result);
    anyhow::ensure!(
        result["status"] != "matched" || severity.is_some(),
        "match has no scored evidence"
    );
    let id = aml_risk_policy::digest(&json!(["kyt_alert_v1", watch_id, lease]));
    store.query(r#"
        MATCH (w:KytWatch {id:$watch_id})
        SET w.lock_version=w.lock_version+1
        WITH w WHERE w.active AND w.lease=$lease
        SET w.result_json=$result,w.last_checked_at_ms=$checked,w.next_check_at_ms=$next,w.lease=null
        FOREACH (_ IN CASE WHEN $no_match THEN [1] ELSE [] END | SET w.match_alert_id=null)
        FOREACH (_ IN CASE WHEN $severity IS NOT NULL AND w.match_alert_id IS NULL THEN [1] ELSE [] END |
            MERGE (a:KytAlert {id:$alert_id})
            ON CREATE SET a.watch_id=w.id,a.owner=w.owner,a.network_id=w.network_id,a.kind=w.kind,
                a.value=w.value,a.system_id=w.system_id,a.customer_id=w.customer_id,a.rule=w.rule,
                a.threshold=w.threshold,a.severity=$severity,a.status='open',a.created_at_ms=$checked,
                a.evidence_json=$result,a.lock_version=0,a.delivery_lock=0,a.delivery_attempts=0,
                a.delivery_status=CASE WHEN $webhook THEN 'pending' ELSE 'disabled' END,
                a.delivery_due_at_ms=CASE WHEN $webhook THEN $checked ELSE null END,
                a.delivery_expires_at_ms=CASE WHEN $webhook THEN $expiry ELSE null END
            SET w.match_alert_id=a.id)
        RETURN w.id
    "#, json!({"watch_id":watch_id,"lease":lease,"alert_id":id,"result":serde_json::to_string(result)?,
        "no_match":result["status"]=="no_match","severity":severity,"checked":checked,"next":next,
        "webhook":webhook_enabled,"expiry":checked+DELIVERY_AGE_MS})).await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<String>,
    status: Option<String>,
    severity: Option<String>,
}

fn cursor(after: &str) -> Option<(i64, &str)> {
    let (time, id) = after.split_once(':')?;
    let time = time.parse::<i64>().ok()?;
    (time >= 0 && watch_id(id)).then_some((time, id))
}

fn item(mut value: Value) -> anyhow::Result<Value> {
    let stored = value
        .as_object_mut()
        .context("invalid KYT alert record")?
        .remove("evidence_json")
        .context("missing KYT alert evidence")?;
    value["evidence"] = serde_json::from_str(stored.as_str().context("invalid KYT evidence")?)?;
    Ok(value)
}

async fn list(
    State(app): State<App>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Response, Failure> {
    let operator = watch_owner(&app, &headers)?;
    let (before, before_id) = match page.after.as_deref() {
        None => (i64::MAX, ""),
        Some(after) => {
            cursor(after).ok_or(Failure(StatusCode::BAD_REQUEST, "Invalid alert cursor"))?
        }
    };
    if page
        .status
        .as_deref()
        .is_some_and(|s| !matches!(s, "open" | "acknowledged" | "resolved"))
        || page
            .severity
            .as_deref()
            .is_some_and(|s| !matches!(s, "low" | "medium" | "high"))
    {
        return Err(Failure(StatusCode::BAD_REQUEST, "Invalid alert filter"));
    }
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MATCH (a:KytAlert)
        WHERE (a.owner=$operator OR a.assignee_hash=$operator)
          AND ($status IS NULL OR a.status=$status) AND ($severity IS NULL OR a.severity=$severity)
          AND (a.created_at_ms < $before OR (a.created_at_ms=$before AND a.id < $before_id))
        RETURN {FIELDS} ORDER BY a.created_at_ms DESC,a.id DESC LIMIT $limit"#
            ),
            json!({"operator":operator.hash,"status":page.status,"severity":page.severity,
            "before":before,"before_id":before_id,"limit":PAGE_SIZE+1}),
        )
        .await
        .map_err(unavailable)?;
    let more = rows.len() > PAGE_SIZE;
    let alerts: Vec<_> = rows
        .into_iter()
        .take(PAGE_SIZE)
        .map(|r| item(r[0].clone()))
        .collect::<anyhow::Result<_>>()
        .map_err(unavailable)?;
    let next = more.then(|| {
        let last = &alerts[PAGE_SIZE - 1];
        format!(
            "{}:{}",
            last["created_at_ms"],
            last["id"].as_str().unwrap_or_default()
        )
    });
    Ok(response(
        json!({"alerts":alerts,"next_after":next}),
        operator,
    ))
}

fn missing() -> Failure {
    Failure(StatusCode::NOT_FOUND, "KYT alert not found")
}

async fn read(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let operator = watch_owner(&app, &headers)?;
    if !watch_id(&id) {
        return Err(missing());
    }
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MATCH (a:KytAlert {{id:$id}})
        WHERE a.owner=$operator OR a.assignee_hash=$operator
        RETURN {FIELDS}"#
            ),
            json!({"id":id,"operator":operator.hash}),
        )
        .await
        .map_err(unavailable)?;
    let row = rows.into_iter().next().ok_or_else(missing)?;
    Ok(response(
        item(row[0].clone()).map_err(unavailable)?,
        operator,
    ))
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Acknowledge,
    Assign { assignee: String },
    Unassign,
    Resolve,
}

fn assignee_name(value: &str) -> bool {
    (1..=96).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._@-".contains(&b))
}

async fn update(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(action): Json<Action>,
) -> Result<Response, Failure> {
    let operator = watch_owner(&app, &headers)?;
    if !watch_id(&id) {
        return Err(missing());
    }
    let username = headers["x-aml-user"]
        .to_str()
        .expect("authenticated operator");
    let (assigning, assignee, permitted, mutation) = match action {
        Action::Assign { assignee } => {
            if !assignee_name(&assignee) {
                return Err(Failure(
                    StatusCode::BAD_REQUEST,
                    "Invalid assignee username",
                ));
            }
            (true, Some(assignee), "a.status <> 'resolved'", ASSIGN)
        }
        Action::Unassign => (true, None, "a.status <> 'resolved'", ASSIGN),
        Action::Acknowledge => (
            false,
            None,
            "true",
            r#"
            SET a.status=CASE WHEN a.status='open' THEN 'acknowledged' ELSE a.status END,
                a.acknowledged_at_ms=coalesce(a.acknowledged_at_ms,$now),
                a.acknowledged_by=coalesce(a.acknowledged_by,$username)"#,
        ),
        Action::Resolve => (
            false,
            None,
            "a.status IN ['acknowledged','resolved']",
            r#"
            SET a.status='resolved',a.resolved_at_ms=coalesce(a.resolved_at_ms,$now),
                a.resolved_by=coalesce(a.resolved_by,$username)"#,
        ),
    };
    let assignee_hash = assignee.as_deref().map(user_owner_hash);
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MATCH (a:KytAlert {{id:$id}})
        WHERE a.owner=$operator OR a.assignee_hash=$operator
        SET a.lock_version=a.lock_version+1
        WITH a WHERE a.owner=$operator OR a.assignee_hash=$operator
        WITH a, CASE WHEN $assigning AND a.owner<>$operator THEN 'forbidden'
                     WHEN NOT ({permitted}) THEN 'conflict' ELSE 'ok' END AS decision
        FOREACH (_ IN CASE WHEN decision='ok' THEN [1] ELSE [] END | {mutation})
        RETURN decision,{FIELDS}"#
            ),
            json!({"id":id,"operator":operator.hash,"username":username,"now":now(),
            "assigning":assigning,"assignee":assignee,"assignee_hash":assignee_hash}),
        )
        .await
        .map_err(unavailable)?;
    let row = rows.into_iter().next().ok_or_else(missing)?;
    match row[0].as_str() {
        Some("ok") => Ok(response(
            item(row[1].clone()).map_err(unavailable)?,
            operator,
        )),
        Some("forbidden") => Err(Failure(
            StatusCode::FORBIDDEN,
            "Only the alert owner can assign it",
        )),
        _ => Err(Failure(
            StatusCode::CONFLICT,
            "Action unavailable in the current alert state",
        )),
    }
}

const ASSIGN: &str = r#"
    FOREACH (change IN CASE WHEN coalesce(a.assignee_hash,'') <> coalesce($assignee_hash,'')
        THEN [1] ELSE [] END |
        SET a.assignee=$assignee,a.assignee_hash=$assignee_hash,a.assigned_at_ms=$now,a.assigned_by=$username)
"#;

async fn backlog(State(app): State<App>, headers: HeaderMap) -> Result<Response, Failure> {
    let operator = watch_owner(&app, &headers)?;
    let rows = app
        .store
        .query(
            r#"
        MATCH (a:KytAlert {owner:$owner})
        RETURN a.delivery_status AS status,count(*) AS total,min(a.delivery_due_at_ms) AS oldest
        ORDER BY status"#,
            json!({"owner":operator.hash}),
        )
        .await
        .map_err(unavailable)?;
    let states: Vec<_> = rows
        .into_iter()
        .map(|r| {
            json!({"status":r[0],"count":r[1],
        "oldest_due_at_ms":r[2]})
        })
        .collect();
    Ok(response(json!({"delivery":states}), operator))
}
