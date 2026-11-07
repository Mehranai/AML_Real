//! Snapshot-fenced cluster production using only the shared schema.
use crate::{
    config::AppConfig,
    db::{Client, database_client, now_ms, row},
};
use aml_warehouse::{Network, visibility};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
const DETECTOR_VERSION: &str = "ethereum_address_clustering_v1";
#[derive(Debug, Clone, Serialize)]
pub struct ClusterDiscoveryReport {
    pub run_id: String,
    pub entity_memberships: u64,
    pub reviewed_entity_claims: u64,
    pub control_claims: u64,
}
pub(crate) fn stable_id(namespace: &str, values: &[&str]) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{namespace}|{}", values.join("|")))
    )
}

pub async fn discover_address_clusters(config: &AppConfig) -> Result<ClusterDiscoveryReport> {
    let client = database_client(config);
    let snapshot = visibility::capture(&client, client.database(), Network::Ethereum).await?;
    if snapshot.state().initialized == 0 {
        return Ok(ClusterDiscoveryReport {
            run_id: "waiting_for_data".into(),
            entity_memberships: 0,
            reviewed_entity_claims: 0,
            control_claims: 0,
        });
    }
    client.invalidate_active("cluster").await?;
    let revision = client.revision("aml_cluster_runs").await?;
    let run_id = format!("ethereum_cluster_{revision}");
    let now = now_ms();
    let result = produce(&client, &snapshot, &run_id, revision, now).await;
    if let Err(error) = &result {
        let failed = row(
            "aml_cluster_runs",
            json!({"run_id":run_id,"status":"failed","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"detector_version":DETECTOR_VERSION,"cursor_json":"{}","started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":0,"error_message":error.to_string()}),
            client.revision("aml_cluster_runs").await?,
            now,
        )?;
        client.insert_rows("aml_cluster_runs", &[failed]).await?;
    }
    result
}
async fn produce(
    client: &Client,
    snapshot: &visibility::Snapshot,
    run: &str,
    rev: u64,
    now: u64,
) -> Result<ClusterDiscoveryReport> {
    let memberships = client
        .rows_from("aml_entity_addresses", "active", "1")
        .await?;
    let claims = client
        .rows_from("aml_intelligence_claims", "active", "claim_kind='label'")
        .await?;
    let by_id = claims
        .iter()
        .filter_map(|c| c["claim_id"].as_str().map(|id| (id, c)))
        .collect::<HashMap<_, _>>();
    let mut grouped = BTreeMap::<String, Vec<&Value>>::new();
    for m in &memberships {
        if let Some(c) = m["claim_id"].as_str().and_then(|id| by_id.get(id)) {
            grouped
                .entry(
                    c["entity_id"]
                        .as_str()
                        .context("entity identity missing")?
                        .into(),
                )
                .or_default()
                .push(c);
        }
    }
    let mut out = BTreeMap::<String, Vec<Value>>::new();
    for name in visibility::family_tables("cluster")? {
        out.insert((*name).into(), vec![]);
    }
    let mut count = 0;
    for (entity, members) in grouped {
        let cluster = stable_id("eth_entity_cluster", &[&entity]);
        let version = format!("{run}:{cluster}");
        let anchor = members
            .iter()
            .filter_map(|m| m["address"].as_str())
            .min()
            .context("empty entity")?;
        for c in &members {
            let address = c["address"].as_str().context("address missing")?;
            let claim = stable_id("eth_cluster_claim", &[run, &cluster, address]);
            out.get_mut("aml_cluster_claims").unwrap().push(row("aml_cluster_claims",json!({"claim_id":claim,"cluster_id":cluster,"subject_id":address,"heuristic":"reviewed_entity","detector_version":DETECTOR_VERSION,"confidence":c["confidence"],"review_status":"approved","source_id":c["source_id"],"evidence_refs":c["evidence_refs"],"evidence_json":json!({"intelligence_claim":c["claim_id"],"claim_revision":c["revision"]}).to_string(),"left_subject_id":anchor,"right_subject_id":address,"same_entity":1,"run_id":run,"source_record_id":c["claim_id"],"created_by":"ethereum_analytics_worker","reviewed_by":c["reviewed_by"],"cluster_type":c["entity_type"],"address_role":c["address_role"]}),rev,now)?);
            out.get_mut("aml_cluster_memberships").unwrap().push(row("aml_cluster_memberships",json!({"cluster_id":cluster,"cluster_version":version,"subject_id":address,"claim_id":claim,"status":"active","membership_id":stable_id("eth_membership",&[run,address,&cluster]),"run_id":run,"entity_id":entity,"membership_type":"REVIEWED_ENTITY","confidence":c["confidence"],"cluster_type":c["entity_type"],"address_role":c["address_role"]}),rev,now)?);
            count += 1;
        }
        out.get_mut("aml_cluster_versions").unwrap().push(row("aml_cluster_versions",json!({"cluster_id":cluster,"cluster_version":version,"cluster_type":"reviewed_entity","display_name":members[0]["entity_name"],"change_type":"refresh","change_reason":"Projection of active reviewed entity claims","source_claim_ids":members.iter().map(|c|c["claim_id"].clone()).collect::<Vec<_>>(),"active_member_count":members.len(),"created_by":"ethereum_analytics_worker","evidence_refs":members.iter().map(|c|format!("claim:{}",c["claim_id"].as_str().unwrap())).collect::<Vec<_>>(),"raw_evidence_json":"{}","run_id":run}),rev,now)?);
    }
    #[derive(serde::Deserialize, clickhouse::Row)]
    struct Deployment {
        tx_hash: String,
        from_address: String,
        contract_address: String,
    }
    let deployments=client.query("SELECT tx_hash,from_address,contract_address FROM eth_transactions WHERE status_known=1 AND status=1 AND contract_address!='' LIMIT 100001").fetch_all::<Deployment>().await?;
    anyhow::ensure!(
        deployments.len() <= 100000,
        "deployment analysis exceeds bounded scope"
    );
    for tx in &deployments {
        out.get_mut("aml_cluster_claims").unwrap().push(row("aml_cluster_claims",json!({"claim_id":stable_id("eth_deployment_control",&[run,&tx.tx_hash]),"cluster_id":format!("control:{}",tx.tx_hash),"subject_id":tx.contract_address,"heuristic":"contract_deployment_control","detector_version":DETECTOR_VERSION,"confidence":1.0,"review_status":"pending","source_id":"ethereum_canonical_ingestion","evidence_refs":[format!("tx:{}",tx.tx_hash)],"evidence_json":json!({"tx_hash":tx.tx_hash}).to_string(),"left_subject_id":tx.from_address,"right_subject_id":tx.contract_address,"same_entity":0,"run_id":run}),rev,now)?);
    }
    let parent = row(
        "aml_cluster_runs",
        json!({"run_id":run,"status":"complete","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"detector_version":DETECTOR_VERSION,"cursor_json":"{}","started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":0,"entity_memberships":count,"control_claims":deployments.len()}),
        rev,
        now,
    )?;
    out.get_mut("aml_cluster_runs").unwrap().push(parent);
    visibility::publish_analysis(
        client,
        client.database(),
        snapshot,
        "cluster",
        run,
        rev,
        &out,
    )
    .await?;
    Ok(ClusterDiscoveryReport {
        run_id: run.into(),
        entity_memberships: count,
        reviewed_entity_claims: count,
        control_claims: deployments.len() as u64,
    })
}
