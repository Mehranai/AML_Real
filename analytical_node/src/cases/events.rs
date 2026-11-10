use super::{
    EVENT_FIELDS, conflict, decode_event, evidence::Reference, missing, request_key, require_id,
    text, unavailable,
};
use crate::{App, Failure, Owner, authenticated_operator, now, response, user_owner_hash};
use aml_chain_api::ApiPath as Path;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    AddNote { text: String },
    AttachInvestigation { investigation_id: String },
    AttachAlert { alert_id: String },
    AttachTransaction { network: String, hash: String },
    AssignReviewer { reviewer: String },
    SubmitReview,
    WithdrawReview,
    RequestChanges { reason: String },
    Approve { comment: Option<String> },
}

#[derive(Deserialize)]
struct StateRecord {
    revision: i64,
    status: String,
    owner: bool,
    reviewer: Option<String>,
    evidence_count: u64,
}

struct Change {
    kind: &'static str,
    status: Option<&'static str>,
    reviewer: Option<String>,
    evidence_key: Option<String>,
    reference_id: Option<String>,
    details: Value,
}

impl Change {
    fn event(kind: &'static str, details: Value) -> Self {
        Self {
            kind,
            status: None,
            reviewer: None,
            evidence_key: None,
            reference_id: None,
            details,
        }
    }

    fn transition(kind: &'static str, status: &'static str, details: Value) -> Self {
        Self {
            status: Some(status),
            ..Self::event(kind, details)
        }
    }
}

fn forbid() -> Failure {
    Failure(
        StatusCode::FORBIDDEN,
        "The operator cannot perform this case action",
    )
}

fn require_role(allowed: bool) -> Result<(), Failure> {
    if !allowed {
        return Err(forbid());
    }
    Ok(())
}

fn require_state(allowed: bool) -> Result<(), Failure> {
    if !allowed {
        return Err(conflict("Action unavailable in the current case state"));
    }
    Ok(())
}

async fn plan(
    command: &Command,
    record: &StateRecord,
    app: &App,
    operator: &Owner,
    actor: &str,
) -> Result<Change, Failure> {
    let editable = matches!(record.status.as_str(), "draft" | "changes_requested");
    let reviewing = record.status == "in_review";
    let reference = match command {
        Command::AttachInvestigation { investigation_id } => {
            Reference::Investigation(investigation_id)
        }
        Command::AttachAlert { alert_id } => Reference::Alert(alert_id),
        Command::AttachTransaction { network, hash } => Reference::Transaction { network, hash },
        Command::AddNote { text: note } => {
            require_state(if record.owner { editable } else { reviewing })?;
            return Ok(Change::event(
                "note_added",
                json!({"text":text(note,4096,true)?}),
            ));
        }
        Command::AssignReviewer { reviewer } => {
            require_role(record.owner)?;
            require_state(editable)?;
            if !(1..=96).contains(&reviewer.len())
                || !reviewer.bytes().all(|b| b.is_ascii_graphic() && b != b':')
                || reviewer == actor
            {
                return Err(Failure(
                    StatusCode::BAD_REQUEST,
                    "Use the distinct reviewer's operator username",
                ));
            }
            return Ok(Change {
                reviewer: Some(reviewer.clone()),
                ..Change::event("reviewer_assigned", json!({"reviewer":reviewer}))
            });
        }
        Command::SubmitReview => {
            require_role(record.owner)?;
            require_state(editable)?;
            if record.reviewer.is_none() || record.evidence_count == 0 {
                return Err(conflict("A reviewer and attached evidence are required"));
            }
            return Ok(Change::transition(
                "review_submitted",
                "in_review",
                json!({"reviewer":record.reviewer}),
            ));
        }
        Command::WithdrawReview => {
            require_role(record.owner)?;
            require_state(reviewing)?;
            return Ok(Change::transition("review_withdrawn", "draft", json!({})));
        }
        Command::RequestChanges { reason } => {
            require_role(!record.owner)?;
            require_state(reviewing)?;
            return Ok(Change::transition(
                "changes_requested",
                "changes_requested",
                json!({"reason":text(reason,4096,true)?}),
            ));
        }
        Command::Approve { comment } => {
            require_role(!record.owner)?;
            require_state(reviewing)?;
            let comment = comment
                .as_deref()
                .map(|comment| text(comment, 4096, true))
                .transpose()?;
            return Ok(Change::transition(
                "review_approved",
                "closed",
                json!({"comment":comment}),
            ));
        }
    };
    require_role(record.owner)?;
    require_state(editable)?;
    let evidence = reference.load(app, &operator.hash).await?;
    Ok(Change {
        evidence_key: Some(evidence.key),
        reference_id: evidence.reference_id,
        ..Change::event(evidence.kind, evidence.details)
    })
}

async fn preflight(
    app: &App,
    operator: &Owner,
    id: &str,
    event_id: &str,
) -> Result<Value, Failure> {
    app.store
        .query(
            &format!(
                r#"
        MATCH (c:InvestigationCase {{id:$id}})
        WHERE c.owner=$operator OR c.reviewer_hash=$operator
        OPTIONAL MATCH (e:CaseEvent {{id:$event_id,case_id:$id}})
        RETURN c{{.status,.revision,.reviewer,.evidence_count,owner:c.owner=$operator}},
            e.request_hash,{EVENT_FIELDS}
        "#
            ),
            json!({"id":id,"operator":operator.hash,"event_id":event_id}),
        )
        .await
        .map_err(unavailable)?
        .into_iter()
        .next()
        .ok_or_else(missing)
}

fn replay(row: &Value, hash: &str) -> Result<Option<Value>, Failure> {
    if row[2].is_null() {
        return Ok(None);
    }
    if row[1] != hash {
        return Err(conflict(
            "Idempotency-Key was used with different case data",
        ));
    }
    Ok(Some(decode_event(row[2].clone()).map_err(unavailable)?))
}

pub(super) async fn append(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(command): Json<Command>,
) -> Result<Response, Failure> {
    let (operator, actor) = authenticated_operator(&app, &headers)?;
    require_id(&id)?;
    let key = request_key(&headers)?;
    let event_id = aml_risk_policy::digest(&json!(["case_event_v1", id, operator.hash, key]));
    let request_hash = aml_risk_policy::digest(&json!(command));
    let row = preflight(&app, &operator, &id, &event_id).await?;
    if let Some(event) = replay(&row, &request_hash)? {
        return Ok(response(event, operator));
    }
    let record: StateRecord = serde_json::from_value(row[0].clone()).map_err(unavailable)?;
    let change = plan(&command, &record, &app, &operator, actor).await?;
    let reviewer_hash = change.reviewer.as_deref().map(user_owner_hash);
    let rows = app.store.case_mutation(&format!(r#"
        MATCH (c:InvestigationCase {{id:$id}})
        WHERE c.owner=$operator OR c.reviewer_hash=$operator
        SET c.lock_version=c.lock_version+1
        WITH c WHERE c.revision=$revision AND (c.owner=$operator OR c.reviewer_hash=$operator)
        OPTIONAL MATCH (duplicate:CaseEvent {{case_id:$id,evidence_key:$evidence_key}})
        OPTIONAL MATCH (i:Investigation {{id:$reference_id,owner:$operator,state:'saved'}})
        OPTIONAL MATCH (a:KytAlert {{id:$reference_id,owner:$operator}})
        WITH c,duplicate,i,a
        WHERE duplicate IS NULL
            AND ($kind <> 'investigation_attached' OR i IS NOT NULL)
            AND ($kind <> 'alert_attached' OR a IS NOT NULL)
        SET c.revision=c.revision+1,c.updated_at_ms=$now,c.status=coalesce($status,c.status),
            c.reviewer=coalesce($reviewer,c.reviewer),c.reviewer_hash=coalesce($reviewer_hash,c.reviewer_hash),
            c.closed_at_ms=CASE WHEN $status='closed' THEN $now ELSE c.closed_at_ms END,
            c.evidence_count=c.evidence_count+CASE WHEN $evidence_key IS NULL THEN 0 ELSE 1 END
        CREATE (e:CaseEvent {{id:$event_id,case_id:c.id,sequence:c.revision,kind:$kind,actor:$actor,
            at_ms:$now,details_json:$details,request_hash:$request_hash,evidence_key:$evidence_key}})
        CREATE (c)-[:HAS_EVENT]->(e)
        FOREACH (_ IN CASE WHEN $kind='investigation_attached' THEN [1] ELSE [] END |
            CREATE (e)-[:REFERS_TO]->(i))
        FOREACH (_ IN CASE WHEN $kind='alert_attached' THEN [1] ELSE [] END |
            CREATE (e)-[:REFERS_TO]->(a))
        RETURN {EVENT_FIELDS}"#),
        json!({"id":id,"operator":operator.hash,"revision":record.revision,"event_id":event_id,
            "request_hash":request_hash,"actor":actor,"now":now(),"kind":change.kind,"status":change.status,
            "reviewer":change.reviewer,"reviewer_hash":reviewer_hash,"evidence_key":change.evidence_key,
            "reference_id":change.reference_id,"details":change.details.to_string()}))
        .await.map_err(unavailable)?;
    if let Some(row) = rows.first() {
        return Ok(response(
            decode_event(row[0].clone()).map_err(unavailable)?,
            operator,
        ));
    }
    let row = preflight(&app, &operator, &id, &event_id).await?;
    let event = replay(&row, &request_hash)?
        .ok_or_else(|| conflict("Case changed or evidence is already attached"))?;
    Ok(response(event, operator))
}
