//! Operational BSC records written into the common schema with actual provenance.
use super::{Client, now_ms, refs, row};
use aml_warehouse::{Network, publication, publication::literal};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
pub fn asset_id(value: &str) -> Result<String> {
    let bare = value.strip_prefix("eip155:56/").unwrap_or(value);
    let asset = if bare.eq_ignore_ascii_case("native:bnb") {
        "native:BNB"
    } else {
        bare
    };
    aml_warehouse::values::AssetId::parse(Network::Bsc, asset)?;
    Ok(asset.into())
}
pub fn collection(standard: &str, token: &str) -> Result<String> {
    asset_id(&format!("{standard}:{token}"))
}
async fn ensure_source(client: &Client, id: &str, reference: &str, actor: &str) -> Result<()> {
    if !client
        .rows(
            "aml_intelligence_sources",
            &format!("source_id={}", literal(id)),
        )
        .await?
        .is_empty()
    {
        return Ok(());
    }
    let source = row(
        "aml_intelligence_sources",
        json!({"source_id":id,"source_name":id,"source_type":"ANALYST","reference_url":reference,"trust_tier":"UNKNOWN","license":"unspecified","is_active":1,"created_by":actor,"metadata_json":"{}"}),
        client.revision("aml_intelligence_sources").await?,
        now_ms(),
    )?;
    client
        .insert_rows("aml_intelligence_sources", &[source])
        .await
}
pub async fn protocol(client: &Client, value: &Value) -> Result<u64> {
    let source = text(value, "source_id");
    let actor = if text(value, "reviewed_by").is_empty() {
        "bsc_protocol_import"
    } else {
        text(value, "reviewed_by")
    };
    ensure_source(client, source, text(value, "source_reference"), actor).await?;
    let revision = client.revision("aml_protocol_registry").await?;
    let record = row(
        "aml_protocol_registry",
        json!({
            "registry_id":format!("bsc:protocol:{}",text(value,"contract_address")),"subject_id":value["contract_address"],"subject_type":"address",
            "protocol":value["protocol"],"protocol_type":value["protocol_type"],"contract_role":value["contract_role"],
            "remote_network_id":if text(value,"remote_network_id").is_empty(){Value::Null}else{value["remote_network_id"].clone()},
            "remote_subject_id":if text(value,"remote_contract_address").is_empty(){Value::Null}else{value["remote_contract_address"].clone()},
            "decoder":value["decoder"],"decoder_config_json":value.to_string(),"source_id":source,"source_reference":value["source_reference"],
            "confidence":value["evidence_confidence"],"enabled":value["enabled"],"review_status":value["review_status"],"reviewed_by":value["reviewed_by"],
            "review_note":value["review_note"],"reviewed_at_unix_ms":if text(value,"review_status")=="pending"{Value::Null}else{json!(now_ms())},
            "evidence_refs":refs(&json!([value["source_reference"]])) ,"raw_evidence_json":value.to_string()
        }),
        revision,
        now_ms(),
    )?;
    client
        .insert_rows("aml_protocol_registry", &[record])
        .await?;
    Ok(revision)
}
pub async fn store(client: &Client, table: &str, values: &[Value]) -> Result<()> {
    ensure!(
        matches!(
            table,
            "intelligence_sources"
                | "intelligence_claims"
                | "token_metadata"
                | "token_metadata_jobs"
        ),
        "unsupported operational write"
    );
    for v in values {
        ensure!(
            v["network_id"] == Network::Bsc.id(),
            "operational network mismatch"
        );
        let now = now_ms();
        match table {
            "intelligence_sources" => {
                let record = row(
                    "aml_intelligence_sources",
                    json!({
                        "source_id":v["source_id"], "source_name":v["source_name"],
                        "source_type":v["source_type"], "reference_url":v["reference_url"],
                        "trust_tier":v["trust_tier"], "license":v["license"],
                        "is_active":u8::from(v["is_active"].as_bool().unwrap_or(false)),
                        "created_by":v["created_by"], "metadata_json":v["metadata_json"]
                    }),
                    client.revision("aml_intelligence_sources").await?,
                    now,
                )?;
                client
                    .insert_rows("aml_intelligence_sources", &[record])
                    .await?;
                for family in ["cluster", "exposure", "risk"] {
                    client.invalidate_active(family).await?;
                }
            }
            "intelligence_claims" => {
                let source = text(v, "source_id");
                let actor = if text(v, "reviewed_by").is_empty() {
                    "bsc_intelligence_import"
                } else {
                    text(v, "reviewed_by")
                };
                ensure_source(client, source, text(v, "source_reference"), actor).await?;
                let revision = client.revision("aml_intelligence_claims").await?;
                let status = text(v, "review_status");
                let record = row(
                    "aml_intelligence_claims",
                    json!({
                        "claim_id":v["claim_id"],"address":v["address"],"claim_kind":v["claim_kind"],"entity_id":v["entity_id"],"entity_name":v["entity_name"],"entity_type":v["entity_type"],
                        "address_role":v["address_role"],"confidence":v["confidence"],"risk_level":v["risk_level"],"is_exposure_seed":u8::from(v["is_exposure_seed"].as_bool().unwrap_or(false)),
                        "seed_category":v["seed_category"],"source_id":source,"source_reference":v["source_reference"],"evidence_refs":refs(&v["evidence_refs"]),
                        "review_status":status,"reviewed_by":v["reviewed_by"],"review_note":v["review_note"],"valid_from_unix_ms":now,"valid_to_unix_ms":null,
                        "reviewed_at_unix_ms":if status=="pending"{Value::Null}else{json!(now)},"raw_evidence_json":v.to_string()
                    }),
                    revision,
                    now,
                )?;
                let review = if status != "pending" {
                    Some(row(
                        "aml_intelligence_reviews",
                        json!({"review_id":format!("bsc:review:{}:{revision}",text(v,"claim_id")),"subject_type":"claim","subject_id":v["claim_id"],"decision":status,"reviewed_by":v["reviewed_by"],"reason":v["review_note"],"evidence_refs":refs(&v["evidence_refs"]),"subject_revision":revision,"raw_evidence_json":v.to_string()}),
                        client.revision("aml_intelligence_reviews").await?,
                        now,
                    )?)
                } else {
                    None
                };
                client
                    .insert_rows("aml_intelligence_claims", &[record])
                    .await?;
                if let Some(review) = review {
                    client
                        .insert_rows("aml_intelligence_reviews", &[review])
                        .await?;
                }
                for family in ["cluster", "exposure", "risk"] {
                    client.invalidate_active(family).await?;
                }
            }
            "token_metadata" => {
                let asset = collection(text(v, "token_standard"), text(v, "token_address"))?;
                let state = publication::state(client, client.database(), Network::Bsc).await?;
                let onchain = text(v, "metadata_source") == "rpc";
                let (observed, hash, revision) = if onchain {
                    let height = v["observed_block"]
                        .as_u64()
                        .context("metadata observed block missing")?;
                    let hash = aml_warehouse::values::normalize_hash(
                        Network::Bsc,
                        text(v, "source_reference"),
                    )?;
                    let actual=client.query("SELECT block_hash FROM aml_ingested_blocks_canonical WHERE network_id='eip155:56' AND block_number=?").bind(height).fetch_optional::<String>().await?;
                    ensure!(
                        actual.as_deref() == Some(hash.as_str()),
                        "metadata source block is no longer canonical"
                    );
                    (json!(height), json!(hash), json!(state.revision))
                } else {
                    (Value::Null, Value::Null, Value::Null)
                };
                let record = row(
                    "aml_asset_metadata",
                    json!({"asset_id":asset,"symbol":v["symbol"],"name":v["name"],"decimals":v["decimals"],"asset_standard":v["token_standard"],"source_id":v["metadata_source"],"source_reference":v["source_reference"],"review_status":if onchain{"pending"}else{"approved"},"reviewed_by":v["reviewed_by"],"evidence_refs":refs(&json!([if onchain{format!("block:{}",hash.as_str().unwrap())}else{text(v,"source_reference").into()}])),"is_verified":v["is_verified"],"metadata_status":v["metadata_status"],"observed_block":observed,"observed_block_hash":hash,"source_revision":revision,"updated_at_unix_ms":now,"raw_evidence_json":v.to_string()}),
                    client.revision("aml_asset_metadata").await?,
                    now,
                )?;
                client.insert_rows("aml_asset_metadata", &[record]).await?;
            }
            "token_metadata_jobs" => {
                let asset = collection(text(v, "token_standard"), text(v, "token_address"))?;
                let attempts = v["attempt_count"]
                    .as_u64()
                    .context("metadata attempts missing")?;
                let state = publication::state(client, client.database(), Network::Bsc).await?;
                let record = row(
                    "aml_metadata_jobs",
                    json!({"job_id":format!("bsc:metadata:{asset}"),"asset_id":asset,"status":if attempts>=10 && v["status"]!="complete"{"dead_letter"}else{text(v,"status")},"attempts":attempts,"next_attempt_unix_ms":v["next_attempt_at_unix_ms"],"last_error":v["last_error_class"],"discovered_block":v["discovered_block"],"source_revision":state.revision,"updated_at_unix_ms":v["updated_at_unix_ms"],"raw_evidence_json":v.to_string()}),
                    client.revision("aml_metadata_jobs").await?,
                    now,
                )?;
                client.insert_rows("aml_metadata_jobs", &[record]).await?;
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}
