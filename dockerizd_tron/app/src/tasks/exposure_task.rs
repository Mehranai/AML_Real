use crate::{
    db::{Client, now_ms, records::operational_lock, row},
    services::tron::exposure::propagation::propagate_exposure,
};
use aml_warehouse::{Network, publication::literal, visibility};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

pub async fn run_exposure_scan(client: Arc<Client>, seed: &str, max_hops: u8) -> Result<usize> {
    let _lock = operational_lock(&client)?;
    let snapshot = visibility::capture(&*client, client.database(), Network::Tron).await?;
    ensure!(
        snapshot.state().initialized == 1,
        "exposure requires indexed history"
    );
    let mut seeds=client.query("SELECT toJSONString(tuple(seed_id,source_label_id,claim_revision)) FROM exposure_seeds WHERE address=? ORDER BY risk_level DESC,seed_id LIMIT 2").bind(seed).fetch_all::<String>().await?;
    ensure!(
        seeds.len() == 1,
        "exposure seed must have exactly one active reviewed claim"
    );
    let identity: Value = serde_json::from_str(&seeds.pop().unwrap())?;
    let seed_id = identity[0].as_str().context("seed identity")?;
    let dependency = identity.to_string();
    for old in client
        .rows_from(
            "aml_exposure_runs",
            "visible",
            &format!("seed_id={}", literal(seed_id)),
        )
        .await?
    {
        visibility::invalidate_analysis(
            &*client,
            client.database(),
            Network::Tron,
            "exposure",
            old["run_id"].as_str().context("run identity")?,
            client.revision("aml_analysis_publications").await?,
        )
        .await?;
    }
    let rev = client.revision("aml_exposure_runs").await?;
    let run = format!("tron_exposure_{rev}");
    let now = now_ms();
    let result = propagate_exposure(&client, seed, max_hops, &run).await;
    let outputs = match result {
        Ok(rows) => rows,
        Err(error) => {
            let failed = row(
                "aml_exposure_runs",
                json!({"run_id":run,"seed_id":seed_id,"status":"failed","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"policy_version":"tron_observed_paths_v1","max_hops":max_hops.clamp(1,10),"started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":1,"error_message":error.to_string(),"seed_snapshot_json":dependency}),
                rev,
                now,
            )?;
            client.insert_rows("aml_exposure_runs", &[failed]).await?;
            return Err(error);
        }
    };
    let mut rows = visibility::family_tables("exposure")?
        .iter()
        .map(|t| (t.to_string(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for output in &outputs {
        let path = &output.best_path;
        let path_id = format!("{run}:{}", output.exposed_address);
        let refs = path
            .tx_hashes
            .iter()
            .map(|tx| format!("tx:{tx}"))
            .collect::<Vec<_>>();
        rows.get_mut("aml_exposure_paths").unwrap().push(row("aml_exposure_paths",json!({"path_id":path_id,"run_id":run,"subject_id":output.exposed_address,"seed_id":seed_id,"direction":"received","hop_count":output.hop_distance,"asset_id":path.asset_id,"amount_raw":path.amount_raw,"amount_share":output.best_path_amount_share,"exposure_score":output.exposure_score,"attribution_method":"observed_directed_path","fund_continuity_claimed":0,"path_subjects":path.subjects,"relationship_ids":path.relationship_ids,"tx_hashes":path.tx_hashes,"evidence_refs":refs,"time_weight":output.best_path_time_weight,"service_mediated":output.service_mediated,"continuity_type":"observed_path_no_allocation","first_transfer_unix_ms":path.first_transfer_unix_ms,"last_transfer_unix_ms":path.last_transfer_unix_ms,"detector_version":"tron_observed_paths_v1","evidence_json":serde_json::to_string(output)?}),rev,now)?);
        rows.get_mut("aml_address_exposures").unwrap().push(row("aml_address_exposures",json!({"exposure_id":path_id,"run_id":run,"seed_id":seed_id,"source_subject_id":seed,"subject_id":output.exposed_address,"hop_distance":output.hop_distance,"exposure_score":output.exposure_score,"path_count":output.path_count,"last_tx_hash":output.last_tx_hash,"last_seen_block":output.last_seen_block,"exposure_type":output.exposure_type,"best_path_amount_share":output.best_path_amount_share,"best_path_time_weight":output.best_path_time_weight,"service_mediated":output.service_mediated,"as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"evidence_refs":refs,"raw_evidence_json":serde_json::to_string(output)?}),rev,now)?);
    }
    rows.get_mut("aml_exposure_runs").unwrap().push(row("aml_exposure_runs",json!({"run_id":run,"seed_id":seed_id,"status":"complete","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"policy_version":"tron_observed_paths_v1","max_hops":max_hops.clamp(1,10),"started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":0,"seed_ids":[seed_id],"seed_snapshot_json":dependency,"detector_version":"tron_observed_paths_v1","hop_decay":0.75,"time_half_life_days":180.0,"seed_count":1,"path_count":outputs.len(),"raw_evidence_json":json!({"source_address":seed,"row_count":outputs.len(),"fund_continuity_claimed":false}).to_string()}),rev,now)?);
    visibility::publish_analysis(
        &*client,
        client.database(),
        &snapshot,
        "exposure",
        &run,
        rev,
        &rows,
    )
    .await?;
    Ok(outputs.len())
}
pub async fn run_all_exposure_scans(client: Arc<Client>, max_hops: u8) -> Result<(usize, usize)> {
    let seeds=client.query("SELECT DISTINCT address FROM exposure_seeds WHERE risk_level>0 AND is_active=1 ORDER BY address").fetch_all::<String>().await?;
    let mut count = 0;
    for seed in &seeds {
        count += run_exposure_scan(client.clone(), seed, max_hops).await?;
    }
    Ok((seeds.len(), count))
}
