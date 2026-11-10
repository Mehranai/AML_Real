use super::{EVENT_FIELDS, conflict, decode_event, missing, require_id, unavailable, valid_id};
use crate::{App, Failure, authenticated_operator, kyt, response};
use aml_chain_api::{ApiPath as Path, Network};
use anyhow::{Context, ensure};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) enum Reference<'a> {
    Investigation(&'a str),
    Alert(&'a str),
    Transaction { network: &'a str, hash: &'a str },
}

pub(super) struct Evidence {
    pub kind: &'static str,
    pub reference_id: Option<String>,
    pub key: String,
    pub details: Value,
}

fn snapshot(raw: &Value, hash: &Value) -> anyhow::Result<Value> {
    let raw = raw
        .as_str()
        .context("missing saved investigation snapshot")?;
    ensure!(
        hash == &format!("{:x}", Sha256::digest(raw.as_bytes())),
        "saved investigation checksum mismatch"
    );
    Ok(serde_json::from_str(raw)?)
}

impl Reference<'_> {
    pub(super) async fn load(self, app: &App, owner: &str) -> Result<Evidence, Failure> {
        match self {
            Self::Investigation(id) => {
                if !(id.len() == 32
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
                {
                    return Err(missing());
                }
                let rows = app.store.query(r#"
                    MATCH (i:Investigation {id:$id,owner:$owner,state:'saved'})
                    RETURN i.payload_json,i{.id,.network_id,.mode,.address,.target,.snapshot_hash,.saved_at_unix_ms}
                "#, json!({"id":id,"owner":owner})).await.map_err(unavailable)?;
                let row = rows.first().ok_or_else(missing)?;
                let stored = &row[1];
                let data = snapshot(&row[0], &stored["snapshot_hash"]).map_err(unavailable)?;
                let risk = &data["risk_engine"];
                Ok(Evidence {
                    kind: "investigation_attached",
                    reference_id: Some(id.into()),
                    key: format!("investigation:{id}"),
                    details: json!({"investigation":stored,"source_snapshot_id":data["source_snapshot"]["snapshot_id"],
                        "source_status_at_capture":data["source_validity"]["status"],
                        "risk_at_capture":{"score":risk["risk_score"],"level":risk["risk_level"],
                            "policy_version":risk["policy_version"]}}),
                })
            }
            Self::Alert(id) => {
                require_id(id)?;
                let rows = app.store.query(r#"
                    MATCH (a:KytAlert {id:$id,owner:$owner})
                    RETURN a{.id,.watch_id,.network_id,.kind,.value,.severity,.rule,.threshold,.created_at_ms},
                        a.evidence_json
                "#, json!({"id":id,"owner":owner})).await.map_err(unavailable)?;
                let row = rows.first().ok_or_else(missing)?;
                let data: Value = serde_json::from_str(
                    row[1]
                        .as_str()
                        .context("missing alert evidence")
                        .map_err(unavailable)?,
                )
                .map_err(unavailable)?;
                Ok(Evidence {
                    kind: "alert_attached",
                    reference_id: Some(id.into()),
                    key: format!("alert:{id}"),
                    details: json!({"alert":row[0],"evidence_at_detection":data}),
                })
            }
            Self::Transaction { network, hash } => {
                let network = Network::ALL
                    .into_iter()
                    .find(|chain| chain.name() == network)
                    .ok_or(Failure(
                        StatusCode::BAD_REQUEST,
                        "Unsupported transaction network",
                    ))?;
                let hash = aml_chain_api::kyt::transaction_hash(network, hash)
                    .map_err(|_| Failure(StatusCode::BAD_REQUEST, "Invalid transaction hash"))?;
                let _permit = app.requests.try_acquire().map_err(|_| {
                    Failure(
                        StatusCode::TOO_MANY_REQUESTS,
                        "Transaction evidence capacity reached; retry shortly",
                    )
                })?;
                let receipt = kyt::read_transaction(app, network, &hash)
                    .await
                    .map_err(|_| {
                        Failure(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "Canonical transaction evidence unavailable",
                        )
                    })?;
                let occurrence = receipt.occurrence.as_ref().ok_or_else(|| {
                    conflict("Transaction has no single canonical indexed occurrence")
                })?;
                Ok(Evidence {
                    kind: "transaction_attached",
                    reference_id: None,
                    key: format!(
                        "transaction:{}:{hash}:{}",
                        network.id(),
                        occurrence.block_hash
                    ),
                    details: json!({"transaction":receipt}),
                })
            }
        }
    }
}

pub(super) async fn read(
    State(app): State<App>,
    headers: HeaderMap,
    Path((id, event_id)): Path<(String, String)>,
) -> Result<Response, Failure> {
    let (operator, _) = authenticated_operator(&app, &headers)?;
    require_id(&id)?;
    if !valid_id(&event_id) {
        return Err(missing());
    }
    let rows = app
        .store
        .query(
            &format!(
                r#"
        MATCH (c:InvestigationCase {{id:$id}})-[:HAS_EVENT]->(e:CaseEvent {{id:$event_id}})
        WHERE (c.owner=$operator OR c.reviewer_hash=$operator) AND e.evidence_key IS NOT NULL
        OPTIONAL MATCH (e)-[:REFERS_TO]->(i:Investigation {{state:'saved'}})
        RETURN {EVENT_FIELDS},i.payload_json
    "#
            ),
            json!({"id":id,"event_id":event_id,"operator":operator.hash}),
        )
        .await
        .map_err(unavailable)?;
    let row = rows.first().ok_or_else(missing)?;
    let event = decode_event(row[0].clone()).map_err(unavailable)?;
    let data = if event["kind"] == "investigation_attached" {
        snapshot(&row[1], &event["details"]["investigation"]["snapshot_hash"])
            .map_err(unavailable)?
    } else {
        Value::Null
    };
    Ok(response(
        json!({"event":event,"historical_investigation":data}),
        operator,
    ))
}
