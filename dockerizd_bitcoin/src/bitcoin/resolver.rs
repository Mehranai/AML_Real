use crate::{
    config::PipelineConfig,
    db::Database,
    model::{self, ScriptOutput},
    rpc::Rpc,
    warehouse,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use tokio::task::JoinSet;

#[derive(Deserialize)]
struct StoredOutput {
    txid: String,
    output_index: u32,
    #[serde(flatten)]
    output: ScriptOutput,
}

pub async fn resolve(
    rpc: &Rpc,
    db: &Database,
    block: &Value,
    config: &PipelineConfig,
) -> Result<HashMap<String, ScriptOutput>> {
    config.validate()?;
    let txs = block["tx"]
        .as_array()
        .context("missing decoded transactions")?;
    let all = txs
        .iter()
        .map(|tx| model::hash(tx["txid"].as_str().context("missing txid")?))
        .collect::<Result<HashSet<_>>>()?;
    let mut earlier = HashSet::new();
    let mut needed = BTreeSet::<(String, u32)>::new();
    for tx in txs {
        for input in tx["vin"].as_array().context("missing vin")? {
            if input.get("coinbase").is_some() {
                continue;
            }
            let id = model::hash(input["txid"].as_str().context("missing previous txid")?)?;
            ensure!(
                !all.contains(&id) || earlier.contains(&id),
                "input refers to a later transaction in the same block"
            );
            if input.get("prevout").is_some() || earlier.contains(&id) {
                continue;
            }
            let n = u32::try_from(input["vout"].as_u64().context("missing vout")?)?;
            needed.insert((id, n));
            ensure!(
                needed.len() <= config.max_prevouts,
                "block exceeds configured prevout work limit"
            );
        }
        earlier.insert(model::hash(tx["txid"].as_str().context("missing txid")?)?);
    }
    let needed: Vec<_> = needed.into_iter().collect();
    let mut found = HashMap::new();
    let mut resolved_bytes = 0usize;
    for chunk in needed.chunks(config.prevout_batch) {
        // Hashes and indices above are validated, not arbitrary SQL literals.
        let keys = chunk
            .iter()
            .map(|(id, n)| format!("('{id}',{n})"))
            .collect::<Vec<_>>()
            .join(",");
        let rows:Vec<StoredOutput>=db.rows(&format!("SELECT txid,output_index,address,script_hex,script_type,value_sats FROM ({}) WHERE (txid,output_index) IN ({keys}) ORDER BY block_height DESC LIMIT 1 BY txid,output_index",warehouse::OUTPUTS),&[]).await?;
        for row in rows {
            charge(&row.output, &mut resolved_bytes, config.max_resolved_bytes)?;
            found.insert(model::outpoint(&row.txid, row.output_index), row.output);
        }
    }
    let missing: Vec<String> = needed
        .iter()
        .filter(|(id, n)| !found.contains_key(&model::outpoint(id, *n)))
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let batches: Vec<_> = missing
        .chunks(config.prevout_batch)
        .map(|c| c.to_vec())
        .collect();
    let mut requested = HashMap::<String, Vec<u32>>::new();
    for (id, n) in &needed {
        if !found.contains_key(&model::outpoint(id, *n)) {
            requested.entry(id.clone()).or_default().push(*n);
        }
    }
    for group in batches.chunks(config.prevout_concurrency) {
        let mut pending = JoinSet::new();
        for ids in group {
            let rpc = rpc.clone();
            let ids = ids.clone();
            pending.spawn(async move {
                let values = rpc
                    .batch(
                        ids.iter()
                            .map(|id| ("getrawtransaction", json!([id, true])))
                            .collect(),
                    )
                    .await?;
                Ok::<_, anyhow::Error>((ids, values))
            });
        }
        while let Some(result) = pending.join_next().await {
            let (ids, values) = result.context("prevout task stopped")??;
            for (id, value) in ids.into_iter().zip(values) {
                ensure!(
                    value["txid"].as_str() == Some(&id),
                    "previous transaction mismatch"
                );
                for n in &requested[&id] {
                    let output = value["vout"]
                        .as_array()
                        .and_then(|v| v.get(*n as usize))
                        .context("historical prevout unavailable; provide undo data or txindex")?;
                    ensure!(
                        output["n"].as_u64() == Some(*n as u64),
                        "previous output index mismatch"
                    );
                    let output = ScriptOutput::from_rpc(output)?;
                    charge(&output, &mut resolved_bytes, config.max_resolved_bytes)?;
                    found.insert(model::outpoint(&id, *n), output);
                }
            }
        }
    }
    for (id, n) in needed {
        let key = model::outpoint(&id, n);
        ensure!(found.contains_key(&key), "unresolved historical prevout");
    }
    Ok(found)
}

fn charge(output: &ScriptOutput, bytes: &mut usize, limit: usize) -> Result<()> {
    *bytes = bytes
        .saturating_add(output.script_hex.len())
        .saturating_add(output.address.len())
        .saturating_add(output.script_type.len())
        .saturating_add(128);
    ensure!(
        *bytes <= limit,
        "resolved prevouts exceed configured evidence byte budget"
    );
    Ok(())
}
