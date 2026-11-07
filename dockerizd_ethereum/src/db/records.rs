//! Typed operational writes into the shared schema; this never imports an old DB.
use super::{Client, now_ms, refs, row};
use aml_warehouse::{Network, publication::literal};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn nullable(v: &Value, k: &str) -> Value {
    let s = text(v, k);
    if s.is_empty() { Value::Null } else { json!(s) }
}
pub fn asset_id(native: &str, token: &str) -> Result<String> {
    let bare = native.strip_prefix("eip155:1/").unwrap_or(native);
    let id = if bare.eq_ignore_ascii_case("native:eth") {
        "native:ETH".to_owned()
    } else if (bare.starts_with("erc721:") || bare.starts_with("erc1155:")) && !token.is_empty() {
        format!("{bare}/{token}")
    } else {
        bare.to_owned()
    };
    aml_warehouse::values::AssetId::parse(Network::Ethereum, &id)?;
    Ok(id)
}
pub fn collection(standard: &str, address: &str) -> Result<String> {
    asset_id(&format!("{}:{address}", standard.to_ascii_lowercase()), "")
}

pub async fn store(client: &Client, kind: &str, v: Value) -> Result<()> {
    let now = now_ms();
    let table = match kind {
        "intelligence_sources" => "aml_intelligence_sources",
        "entity_labels" => "aml_intelligence_claims",
        "intelligence_reviews" => "aml_intelligence_reviews",
        "address_entities" => "aml_entity_addresses",
        _ => anyhow::bail!("unsupported operational record: {kind}"),
    };
    let rev = client.revision(table).await?;
    let source = serde_json::to_string(&v)?;
    let data = match kind {
        "intelligence_sources" => {
            json!({"source_id":v["source_id"],"source_name":v["source_name"],"source_type":v["source_type"],"reference_url":v["reference_url"],"trust_tier":v["trust_tier"],"license":"unspecified","is_active":v["is_active"],"created_by":v["created_by"],"metadata_json":"{}","raw_evidence_json":source})
        }
        "entity_labels" => {
            json!({"claim_id":v["label_id"],"address":v["address"],"claim_kind":"label","entity_id":v["entity_id"],"entity_name":v["entity_name"],"entity_type":v["entity_type"],"address_role":text(&v,"address_role").to_ascii_lowercase(),"confidence":v["confidence"],"risk_level":v["risk_level"],"is_exposure_seed":v["is_exposure_seed"],"seed_category":v["seed_category"],"source_id":v["source_id"],"source_reference":v["source_record_id"],"evidence_refs":refs(&v["evidence_refs"]),"review_status":"pending","reviewed_by":"","review_note":"","valid_from_unix_ms":now,"valid_to_unix_ms":null,"source_record_id":nullable(&v,"source_record_id"),"supersedes_claim_id":nullable(&v,"supersedes_label_id"),"submitted_by":nullable(&v,"submitted_by"),"case_id":nullable(&v,"case_id"),"raw_evidence_json":source})
        }
        "intelligence_reviews" => {
            let id = text(&v, "subject_id");
            let mut claims = client
                .rows(
                    "aml_intelligence_claims",
                    &format!("claim_id={}", literal(id)),
                )
                .await?;
            let mut claim = claims
                .pop()
                .context("review has no corresponding current claim")?;
            let decision = text(&v, "decision").to_ascii_lowercase();
            let claim_rev = client.revision("aml_intelligence_claims").await?;
            claim["review_status"] = json!(decision);
            claim["reviewed_by"] = v["reviewer"].clone();
            claim["review_note"] = v["reason"].clone();
            claim["reviewed_at_unix_ms"] = json!(now);
            claim["revision"] = json!(claim_rev);
            ensure!(
                !text(&v, "reviewer").is_empty() && !text(&v, "reason").is_empty(),
                "review actor/reason required"
            );
            client
                .insert_rows("aml_intelligence_claims", &[claim])
                .await?;
            json!({"review_id":v["review_id"],"subject_type":"claim","subject_id":id,"decision":decision,"reviewed_by":v["reviewer"],"reason":v["reason"],"evidence_refs":refs(&v["evidence_refs"]),"subject_revision":claim_rev,"raw_evidence_json":source})
        }
        "address_entities" => {
            let id = text(&v, "source_label_id");
            let mut claims = client
                .rows(
                    "aml_intelligence_claims",
                    &format!("claim_id={}", literal(id)),
                )
                .await?;
            let claim = claims.pop().context("entity address has no claim")?;
            ensure!(
                claim["review_status"] == "approved",
                "entity address claim must be approved"
            );
            let reviewed = claim["reviewed_by"].clone();
            let entity = row(
                "aml_entities",
                json!({"entity_id":v["entity_id"],"entity_name":v["entity_name"],"entity_type":v["entity_type"],"source_id":claim["source_id"],"source_reference":claim["source_reference"],"is_active":v["is_active"],"review_status":"approved","reviewed_by":reviewed,"reviewed_at_unix_ms":now,"review_note":claim["review_note"],"evidence_refs":claim["evidence_refs"],"raw_evidence_json":source}),
                client.revision("aml_entities").await?,
                now,
            )?;
            client.insert_rows("aml_entities", &[entity]).await?;
            json!({"entity_id":v["entity_id"],"address":v["address"],"address_role":text(&v,"address_role").to_ascii_lowercase(),"claim_id":id,"source_id":claim["source_id"],"confidence":v["confidence"],"is_active":v["is_active"],"first_seen_block":null,"last_seen_block":null,"review_status":"approved","reviewed_by":reviewed,"reviewed_at_unix_ms":now,"review_note":claim["review_note"],"evidence_refs":claim["evidence_refs"],"raw_evidence_json":source})
        }
        _ => unreachable!(),
    };
    client
        .insert_rows(table, &[row(table, data, rev, now)?])
        .await?;
    if kind == "intelligence_reviews" || kind == "intelligence_sources" {
        for family in ["cluster", "exposure", "risk"] {
            client.invalidate_active(family).await?;
        }
    }
    Ok(())
}

pub async fn metadata_job(client: &Client, v: &crate::storage::TokenMetadataJobRow) -> Result<()> {
    let asset = collection(&v.token_standard, &v.token_address)?;
    let state =
        aml_warehouse::publication::state(client, client.database(), Network::Ethereum).await?;
    let status = match v.status.as_str() {
        "failed" => "dead_letter",
        "completed" => "complete",
        s => s,
    };
    let data = row(
        "aml_metadata_jobs",
        json!({"job_id":format!("ethereum:metadata:{asset}"),"asset_id":asset,"status":status,"attempts":v.attempt_count,"next_attempt_unix_ms":0,"last_error":v.last_error,"discovered_block":v.discovered_block,"source_revision":state.revision,"updated_at_unix_ms":v.updated_at_unix_ms,"raw_evidence_json":serde_json::to_string(v)?}),
        client.revision("aml_metadata_jobs").await?,
        now_ms(),
    )?;
    client.insert_rows("aml_metadata_jobs", &[data]).await
}
pub async fn metadata(
    client: &Client,
    m: &crate::storage::TokenMetadataRow,
    j: &crate::storage::TokenMetadataJobRow,
) -> Result<()> {
    let asset = collection(&m.token_standard, &m.token_address)?;
    let state =
        aml_warehouse::publication::state(client, client.database(), Network::Ethereum).await?;
    let hash=client.query("SELECT block_hash FROM aml_ingested_blocks_canonical WHERE network_id='eip155:1' AND block_number=?").bind(j.discovered_block).fetch_optional::<String>().await?.context("metadata discovery block is not canonical")?;
    ensure!(
        m.observed_block == j.discovered_block && m.observed_block_hash == hash,
        "metadata source block changed before storage; retry at the canonical block"
    );
    let data = row(
        "aml_asset_metadata",
        json!({"asset_id":asset,"symbol":m.symbol,"name":m.name,"decimals":if m.decimals_known==1{json!(m.decimals)}else{Value::Null},"asset_standard":m.token_standard,"source_id":m.metadata_source,"source_reference":format!("block:{hash}"),"review_status":"pending","reviewed_by":"","evidence_refs":[format!("block:{hash}")],"total_supply_raw":if m.total_supply.is_empty(){Value::Null}else{json!(m.total_supply)},"code_hash":if m.code_hash.is_empty(){Value::Null}else{json!(m.code_hash)},"is_verified":m.is_verified,"metadata_status":m.metadata_status,"observed_block":j.discovered_block,"source_revision":state.revision,"observed_block_hash":hash,"updated_at_unix_ms":now_ms(),"raw_evidence_json":serde_json::to_string(m)?}),
        client.revision("aml_asset_metadata").await?,
        now_ms(),
    )?;
    client.insert_rows("aml_asset_metadata", &[data]).await?;
    metadata_job(client, j).await
}
pub async fn initialize_policy(client: &Client) -> Result<()> {
    const POLICY: &str = "ethereum_evidence_policy_v1";
    let now = now_ms();
    if client.query("SELECT count() FROM aml_risk_policies_current WHERE network_id='eip155:1' AND policy_version=?").bind(POLICY).fetch_one::<u64>().await?==0 {
    let p=row("aml_risk_policies",json!({"policy_version":POLICY,"policy_name":"Ethereum explainable AML evidence policy v1","enabled":1,"medium_threshold":40.0,"high_threshold":70.0}),client.revision("aml_risk_policies").await?,now)?;
    client.insert_rows("aml_risk_policies",&[p]).await?;
 }
    for (i, (signal, weight, description)) in [
        ("CONFIRMED_ILLICIT_SEED", 100.0, "Approved exposure seed"),
        ("DIRECT_RECEIVED_EXPOSURE", 45.0, "Direct received exposure"),
        ("DIRECT_SENT_EXPOSURE", 30.0, "Direct sent exposure"),
        ("INDIRECT_EXPOSURE", 25.0, "Indirect exposure"),
        ("MIXER_INTERACTION", 35.0, "Mixer interaction"),
        ("MIXER_BRIDGE_SEQUENCE", 25.0, "Mixer and bridge sequence"),
        (
            "RAPID_SWAP_BRIDGE_SEQUENCE",
            15.0,
            "Rapid swap and bridge sequence",
        ),
        ("RAPID_PASS_THROUGH", 20.0, "Same-asset pass-through"),
    ]
    .iter()
    .enumerate()
    {
        let id = format!("R{:03}", i + 1);
        if client.query("SELECT count() FROM aml_risk_policy_rules_current WHERE network_id='eip155:1' AND policy_version=? AND rule_id=?").bind(POLICY).bind(&id).fetch_one::<u64>().await?==0 {
      let rule=row("aml_risk_policy_rules",json!({"policy_version":POLICY,"rule_id":id,"signal_type":signal,"max_contribution":weight,"description":description,"enabled":1}),client.revision("aml_risk_policy_rules").await?,now)?;
      client.insert_rows("aml_risk_policy_rules",&[rule]).await?;
    }
    }
    initialize_registry(client).await
}

async fn initialize_registry(client: &Client) -> Result<()> {
    // This is the reviewed configuration shipped by the project, with an explicit
    // software actor. Existing disabled sources/records are never reactivated.
    if client.query("SELECT count() FROM aml_intelligence_sources_current WHERE network_id='eip155:1' AND source_id='ethereum_builtin_registry'").fetch_one::<u64>().await?==0 {
  store(client,"intelligence_sources",json!({"source_id":"ethereum_builtin_registry","source_name":"Ethereum bundled protocol registry","source_type":"INTERNAL","trust_tier":"VERIFIED","reference_url":"https://github.com/Mehranai/AML_Real","is_active":1,"created_by":"ethereum_registry_release"})).await?;
 }
    for entry in include_str!("../../config/protocols.json")
        .parse::<Value>()?
        .as_array()
        .context("invalid bundled registry")?
    {
        let id = text(entry, "contract_address");
        if client.query("SELECT count() FROM aml_protocol_registry_current WHERE network_id='eip155:1' AND registry_id=?").bind(id).fetch_one::<u64>().await?==0 {
   let record=row("aml_protocol_registry",json!({"registry_id":id,"subject_id":id,"subject_type":"contract","protocol":entry["protocol"],"protocol_type":entry["protocol_type"],"contract_role":entry["contract_role"],"remote_network_id":nullable(entry,"remote_network_id"),"remote_subject_id":nullable(entry,"remote_contract_address"),"decoder":entry["decoder"],"decoder_config_json":"{}","source_id":"ethereum_builtin_registry","source_reference":entry["source"],"confidence":entry["confidence"],"enabled":entry["enabled"],"review_status":"approved","reviewed_by":"ethereum_registry_release","reviewed_at_unix_ms":now_ms(),"review_note":"Bundled release configuration; not a human analyst decision","evidence_refs":refs(&json!([entry["source"]])),"raw_evidence_json":serde_json::to_string(entry)?}),client.revision("aml_protocol_registry").await?,now_ms())?;
   client.insert_rows("aml_protocol_registry",&[record]).await?;
  }
    }
    Ok(())
}
