use anyhow::{Context, ensure};
use serde_json::{Value, json};

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
        ensure!(
            body["errors"].as_array().is_none_or(|e| e.is_empty()),
            "Neo4j query failed: {}",
            body["errors"]
        );
        ensure!(status.is_success(), "Neo4j HTTP request failed: {status}");
        Ok(body
            .pointer("/data/values")
            .and_then(Value::as_array)
            .context("Neo4j returned no result values")?
            .clone())
    }

    pub async fn initialize(&self) -> anyhow::Result<()> {
        for query in [
            "CREATE CONSTRAINT investigation_id IF NOT EXISTS FOR (i:Investigation) REQUIRE i.id IS UNIQUE",
            "CREATE CONSTRAINT investigation_wallet_key IF NOT EXISTS FOR (w:InvestigationWallet) REQUIRE w.key IS UNIQUE",
            "CREATE INDEX investigation_expiry IF NOT EXISTS FOR (i:Investigation) ON (i.state, i.expires_at_unix_ms)",
            "CREATE INDEX investigation_owner IF NOT EXISTS FOR (i:Investigation) ON (i.owner, i.state, i.created_at_unix_ms)",
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
                MATCH (i)-[:CONTAINS]->(w:InvestigationWallet)
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
