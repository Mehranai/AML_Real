use anyhow::{Context, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug)]
struct QueryErrors(Value);

impl std::fmt::Display for QueryErrors {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Neo4j query failed: {}", self.0)
    }
}

impl std::error::Error for QueryErrors {}

#[derive(Clone)]
pub struct Store {
    client: reqwest::Client,
    endpoint: String,
    password: String,
}

impl Store {
    pub fn new(client: reqwest::Client, url: &str, password: String) -> Self {
        Self {
            client,
            endpoint: format!("{}/db/neo4j/query/v2", url.trim_end_matches('/')),
            password,
        }
    }

    pub async fn query(&self, statement: &str, parameters: Value) -> anyhow::Result<Vec<Value>> {
        let response = self
            .client
            .post(&self.endpoint)
            .basic_auth("neo4j", Some(&self.password))
            .json(&json!({"statement": statement, "parameters": parameters}))
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await?;
        // Neo4j can return HTTP 202 even when a Cypher transaction has failed.
        if body["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(QueryErrors(body["errors"].clone()).into());
        }
        ensure!(status.is_success(), "Neo4j HTTP request failed: {status}");
        Ok(body
            .pointer("/data/values")
            .and_then(Value::as_array)
            .context("Neo4j returned no result values")?
            .clone())
    }

    /// Case mutations can safely replay a transaction that Neo4j rolled back on lock contention.
    pub async fn case_mutation(
        &self,
        statement: &str,
        parameters: Value,
    ) -> anyhow::Result<Vec<Value>> {
        for delay_ms in [20, 50] {
            match self.query(statement, parameters.clone()).await {
                Err(error)
                    if error.downcast_ref::<QueryErrors>().is_some_and(|errors| {
                        errors.0.as_array().into_iter().flatten().any(|entry| {
                            matches!(
                                entry["code"].as_str(),
                                Some(
                                    "Neo.TransientError.Transaction.DeadlockDetected"
                                        | "Neo.TransientError.Transaction.LockAcquisitionTimeout"
                                )
                            )
                        })
                    }) =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await
                }
                result => return result,
            }
        }
        self.query(statement, parameters).await
    }

    pub async fn initialize(&self) -> anyhow::Result<()> {
        for query in [
            "CREATE CONSTRAINT investigation_id IF NOT EXISTS FOR (i:Investigation) REQUIRE i.id IS UNIQUE",
            "CREATE CONSTRAINT investigation_wallet_key IF NOT EXISTS FOR (w:InvestigationWallet) REQUIRE w.key IS UNIQUE",
            "CREATE CONSTRAINT bridge_occurrence_key IF NOT EXISTS FOR (w:BridgeOccurrence) REQUIRE w.key IS UNIQUE",
            "CREATE INDEX investigation_expiry IF NOT EXISTS FOR (i:Investigation) ON (i.state, i.expires_at_unix_ms)",
            "CREATE INDEX investigation_owner IF NOT EXISTS FOR (i:Investigation) ON (i.owner, i.state, i.created_at_unix_ms)",
            "CREATE CONSTRAINT kyt_watch_id IF NOT EXISTS FOR (w:KytWatch) REQUIRE w.id IS UNIQUE",
            "CREATE INDEX kyt_watch_due IF NOT EXISTS FOR (w:KytWatch) ON (w.active, w.next_check_at_ms)",
            "CREATE INDEX kyt_watch_owner IF NOT EXISTS FOR (w:KytWatch) ON (w.owner, w.active, w.id)",
            "CREATE CONSTRAINT kyt_alert_id IF NOT EXISTS FOR (a:KytAlert) REQUIRE a.id IS UNIQUE",
            "CREATE INDEX kyt_alert_owner IF NOT EXISTS FOR (a:KytAlert) ON (a.owner, a.created_at_ms)",
            "CREATE INDEX kyt_alert_assignee IF NOT EXISTS FOR (a:KytAlert) ON (a.assignee_hash, a.created_at_ms)",
            "CREATE INDEX kyt_delivery_due IF NOT EXISTS FOR (a:KytAlert) ON (a.delivery_status, a.delivery_due_at_ms)",
            "CREATE INDEX kyt_delivery_expiry IF NOT EXISTS FOR (a:KytAlert) ON (a.delivery_expires_at_ms)",
            "CREATE CONSTRAINT aml_case_id IF NOT EXISTS FOR (c:InvestigationCase) REQUIRE c.id IS UNIQUE",
            "CREATE INDEX aml_case_owner IF NOT EXISTS FOR (c:InvestigationCase) ON (c.owner, c.id)",
            "CREATE INDEX aml_case_reviewer IF NOT EXISTS FOR (c:InvestigationCase) ON (c.reviewer_hash, c.id)",
            "CREATE CONSTRAINT aml_case_event_id IF NOT EXISTS FOR (e:CaseEvent) REQUIRE e.id IS UNIQUE",
            "CREATE CONSTRAINT aml_case_event_sequence IF NOT EXISTS FOR (e:CaseEvent) REQUIRE (e.case_id,e.sequence) IS UNIQUE",
            "CREATE CONSTRAINT aml_case_evidence_key IF NOT EXISTS FOR (e:CaseEvent) REQUIRE (e.case_id,e.evidence_key) IS UNIQUE",
        ] {
            self.query(query, json!({})).await?;
        }
        Ok(())
    }

    pub async fn create(&self, params: Value) -> anyhow::Result<()> {
        self.query(
            r#"
            CREATE (i:Investigation)
            SET i = $metadata, i.owner = $owner, i.payload_json = $payload, i.lock_version = 0
            WITH i
            CALL {
                WITH i
                UNWIND $nodes AS node
                CREATE (w:InvestigationWallet)
                SET w = node, w.investigation_id = i.id, w.network_id = i.network_id
                CREATE (i)-[:CONTAINS]->(w)
                RETURN count(*) AS node_count
            }
            CALL {
                WITH i
                UNWIND $edges AS edge
                MATCH (a:InvestigationWallet {key: edge.from_key})
                MATCH (b:InvestigationWallet {key: edge.to_key})
                CREATE (a)-[r:TRANSFER]->(b)
                SET r = edge, r.investigation_id = i.id, r.network_id = i.network_id
                RETURN count(*) AS edge_count
            }
            RETURN i.id, node_count, edge_count
        "#,
            params,
        )
        .await?;
        Ok(())
    }

    pub async fn read(&self, id: &str, owner: &str, now: i64) -> anyhow::Result<Option<Value>> {
        let rows = self.query(r#"
            MATCH (i:Investigation {id: $id, owner: $owner})
            WHERE i.state = 'saved' OR i.expires_at_unix_ms > $now
            RETURN i.payload_json, i { .id, .network_id, .network, .mode, .address, .target,
                .state, .created_at_unix_ms, .expires_at_unix_ms, .saved_at_unix_ms, .snapshot_hash }
        "#, json!({"id":id,"owner":owner,"now":now})).await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let mut payload: Value =
            serde_json::from_str(row[0].as_str().context("invalid snapshot JSON")?)?;
        payload["investigation"] = row[1].clone();
        Ok(Some(payload))
    }

    pub async fn export(&self, id: &str, owner: &str, now: i64) -> anyhow::Result<Option<Value>> {
        let rows = self.query(r#"
            MATCH (i:Investigation {id: $id, owner: $owner})
            SET i.lock_version = i.lock_version + 1
            WITH i WHERE i.state = 'saved' OR i.expires_at_unix_ms > $now
            SET i.state = 'saved', i.saved_at_unix_ms = coalesce(i.saved_at_unix_ms, $now)
            REMOVE i.expires_at_unix_ms
            RETURN i { .id, .network_id, .network, .mode, .address, .target,
                .state, .created_at_unix_ms, .expires_at_unix_ms, .saved_at_unix_ms, .snapshot_hash }
        "#, json!({"id":id,"owner":owner,"now":now})).await?;
        Ok(rows.first().map(|row| row[0].clone()))
    }

    pub async fn record_bridge(
        &self,
        id: &str,
        owner: &str,
        now: i64,
        mut trace: Value,
    ) -> anyhow::Result<Option<Value>> {
        let endpoint = |side| {
            let leg = &trace[side];
            let mut endpoint = json!({
                "network_id":leg["network_id"],"tx_hash":leg["receipt"]["tx_hash"],
                "block_hash":leg["receipt"]["block_hash"],"block_number":leg["receipt"]["block_number"],
                "log_index":leg["receipt"]["log_index"]
            });
            for field in ["output_index", "input_index"] {
                if let Some(index) = leg["receipt"][field].as_u64() {
                    endpoint[field] = json!(index);
                }
            }
            endpoint
        };
        let from = endpoint("send");
        let to = endpoint("relay");
        let fingerprint = json!([id, trace["policy"], trace["operation_id"], from, to]);
        let trace_id = format!("{:x}", Sha256::digest(serde_json::to_vec(&fingerprint)?));
        trace["id"] = json!(trace_id);
        let rows = self.query(r#"
            MATCH (i:Investigation {id:$id, owner:$owner})
            SET i.lock_version = i.lock_version + 1
            WITH i WHERE i.state = 'saved' OR i.expires_at_unix_ms > $now
            OPTIONAL MATCH (i)-[:CONTAINS]->(:BridgeOccurrence)-[existing:BRIDGE_OPERATION]->(:BridgeOccurrence)
            WITH i, count(existing) AS traces, collect(existing.id) AS trace_ids
            WHERE traces < 32 OR $trace_id IN trace_ids
            MERGE (a:BridgeOccurrence {key:$send_key})
            ON CREATE SET a += $send
            MERGE (b:BridgeOccurrence {key:$relay_key})
            ON CREATE SET b += $relay
            MERGE (i)-[:CONTAINS]->(a)
            MERGE (i)-[:CONTAINS]->(b)
            MERGE (a)-[r:BRIDGE_OPERATION {id:$trace_id}]->(b)
            ON CREATE SET r.evidence_json=$evidence, r.operation_id=$operation_id, r.confidence=$confidence
            RETURN r.evidence_json
        "#, json!({
            "id":id,"owner":owner,"now":now,"trace_id":trace_id,
            "send_key":format!("{trace_id}:send"),"relay_key":format!("{trace_id}:relay"),
            "send":from,"relay":to,"evidence":serde_json::to_string(&trace)?,
            "operation_id":trace["operation_id"],"confidence":trace["confidence"]
        })).await?;
        rows.first()
            .map(|row| {
                Ok(serde_json::from_str(
                    row[0].as_str().context("invalid bridge evidence")?,
                )?)
            })
            .transpose()
    }

    pub async fn bridge_traces(
        &self,
        id: &str,
        owner: &str,
        now: i64,
    ) -> anyhow::Result<Option<Vec<Value>>> {
        let rows = self.query(r#"
            MATCH (i:Investigation {id:$id, owner:$owner})
            WHERE i.state = 'saved' OR i.expires_at_unix_ms > $now
            OPTIONAL MATCH (i)-[:CONTAINS]->(:BridgeOccurrence)-[r:BRIDGE_OPERATION]->(:BridgeOccurrence)
            WITH i, r ORDER BY r.id
            RETURN i.id, collect(r.evidence_json)
        "#, json!({"id":id,"owner":owner,"now":now})).await?;
        rows.first()
            .map(|row| {
                row[1]
                    .as_array()
                    .context("invalid bridge evidence list")?
                    .iter()
                    .map(|entry| {
                        Ok(serde_json::from_str(
                            entry.as_str().context("invalid bridge evidence")?,
                        )?)
                    })
                    .collect()
            })
            .transpose()
    }

    pub async fn list(&self, owner: &str, before: i64) -> anyhow::Result<Vec<Value>> {
        let rows = self.query(r#"
            MATCH (i:Investigation {owner: $owner, state: 'saved'})
            WHERE i.created_at_unix_ms < $before
            RETURN i { .id, .network_id, .network, .mode, .address, .target, .state, .created_at_unix_ms,
                .saved_at_unix_ms, .snapshot_hash }
            ORDER BY i.created_at_unix_ms DESC LIMIT 100
        "#, json!({"owner":owner,"before":before})).await?;
        Ok(rows.into_iter().map(|row| row[0].clone()).collect())
    }

    pub async fn cleanup(&self, now: i64) -> anyhow::Result<()> {
        // Lock and recheck after acquisition: Export must not race an expiry decision.
        self.query(
            r#"
            MATCH (i:Investigation {state: 'temporary'})
            WHERE i.expires_at_unix_ms <= $now
            WITH i ORDER BY i.expires_at_unix_ms LIMIT 10
            SET i.lock_version = i.lock_version + 1
            WITH i WHERE i.state = 'temporary' AND i.expires_at_unix_ms <= $now
            CALL {
                WITH i
                MATCH (i)-[:CONTAINS]->(w)
                DETACH DELETE w
                RETURN count(*) AS removed
            }
            DETACH DELETE i RETURN count(*) AS investigations_removed
        "#,
            json!({"now":now}),
        )
        .await?;
        Ok(())
    }
}
