//! SQL projections for Ethereum DTOs over shared canonical/current views.
//! These CTEs are query-local: they create no tables, views or duplicate data.
use std::collections::BTreeSet;
pub fn expand(sql: &str) -> String {
    let mut names = BTreeSet::new();
    for word in sql.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if word.starts_with("eth_") {
            names.insert(word);
        }
    }
    let mut items = Vec::new();

    if names.contains("eth_transactions") {
        items.push(r##"eth_transactions AS (SELECT network_id,tx_hash,block_hash,block_number,block_timestamp_unix_ms,transaction_index,
 ifNull(from_address,'') AS from_address,ifNull(to_address,'') AS to_address,
 JSONExtractString(raw_evidence_json,'contract_address') AS contract_address,
 JSONExtractUInt(raw_evidence_json,'nonce') AS nonce,toUInt8(JSONExtractUInt(raw_evidence_json,'transaction_type')) AS transaction_type,
 ifNull(amount_raw,toUInt256(0)) AS value,JSONExtractString(raw_evidence_json,'input_selector') AS input_selector,
 JSONExtractString(raw_evidence_json,'input_data') AS input_data,toUInt8(t.status='success') AS status,toUInt8(t.status IN ('success','failed')) AS status_known,
 JSONExtractUInt(raw_evidence_json,'gas_limit') AS gas_limit,JSONExtractUInt(raw_evidence_json,'gas_used') AS gas_used,fee_raw AS fee_paid,
 created_at_unix_ms FROM aml_transactions_canonical AS t)"##);
    }
    if names.contains("eth_relationships") {
        items.push(r##"eth_relationships AS (SELECT relationship_id,network_id,block_hash,block_number,block_timestamp_unix_ms,tx_hash,transaction_index,event_index,event_sub_index,trace_address,
 ifNull(from_address,'') AS from_address,ifNull(to_address,'') AS to_address,
 if(asset_id='native:ETH',concat(network_id,'/native:eth'),concat(network_id,'/',arrayElement(splitByChar('/',asset_id),1))) AS asset_id,
 ifNull(toString(token_id),'') AS token_id,amount_raw AS amount,transfer_type FROM aml_address_relationships_canonical)"##);
    }
    if names.contains("eth_events") {
        items.push(r##"eth_events AS (SELECT event_id,network_id,tx_hash,block_number,block_timestamp_unix_ms,event_index,event_type,subject_id AS subject_address,protocol,
 JSONExtractString(raw_evidence_json,'protocol_contract') AS protocol_contract,JSONExtractString(raw_evidence_json,'counterparty_address') AS counterparty_address,
 JSONExtractString(raw_evidence_json,'asset_in') AS asset_in,JSONExtractString(raw_evidence_json,'asset_out') AS asset_out,
 ifNull(remote_network_id,'') AS remote_network_id,JSONExtractString(raw_evidence_json,'remote_asset') AS remote_asset,
 JSONExtractString(raw_evidence_json,'bridge_direction') AS bridge_direction,JSONExtractString(raw_evidence_json,'correlation_key') AS correlation_key,
 ifNull(toString(amount_in),'') AS amount_in,ifNull(toString(amount_out),'') AS amount_out,detector,detector_version,toFloat32(confidence) AS confidence,evidence_json FROM aml_semantic_aml_events_canonical)"##);
    }
    if names.contains("eth_metadata") {
        items.push(r##"eth_metadata AS (SELECT network_id,arrayElement(splitByChar(':',asset_id),-1) AS token_address,asset_standard AS token_standard,name,symbol,
 ifNull(m.decimals,toUInt8(0)) AS decimals,toUInt8(isNotNull(m.decimals)) AS decimals_known,ifNull(toString(total_supply_raw),'') AS total_supply,
 ifNull(code_hash,'') AS code_hash,ifNull(is_verified,toUInt8(0)) AS is_verified,source_id AS metadata_source,metadata_status,created_at_unix_ms FROM aml_asset_metadata_visible AS m)"##);
    }
    if names.contains("eth_metadata_jobs") {
        items.push(r##"eth_metadata_jobs AS (SELECT network_id,arrayElement(splitByChar(':',asset_id),-1) AS token_address,arrayElement(splitByChar(':',asset_id),1) AS token_standard,
 ifNull(discovered_block,toUInt64(0)) AS discovered_block,status,toUInt8(least(attempts,255)) AS attempt_count,last_error,ifNull(updated_at_unix_ms,created_at_unix_ms) AS updated_at_unix_ms FROM aml_metadata_jobs_visible
 UNION ALL SELECT network_id,arrayElement(splitByChar(':',asset_id),-1) AS token_address,standard_hint AS token_standard,block_number AS discovered_block,'pending' AS status,toUInt8(0) AS attempt_count,'' AS last_error,created_at_unix_ms AS updated_at_unix_ms
 FROM aml_metadata_discoveries_canonical WHERE (network_id,asset_id) NOT IN (SELECT network_id,asset_id FROM aml_metadata_jobs_current))"##);
    }
    if names.contains("eth_blocks") {
        items.push(r##"eth_blocks AS (SELECT *,ifNull(receipt_coverage,toUInt8(0)) AS receipt_data_complete,ifNull(trace_coverage,toUInt8(0)) AS trace_data_complete,revision AS updated_at,created_at_unix_ms AS indexed_at_unix_ms FROM aml_ingested_blocks_canonical)"##);
    }
    if names.contains("eth_sync") {
        items.push(r##"eth_sync AS (SELECT *,last_block_number AS last_synced_block,last_block_hash AS last_synced_block_hash,published_at_unix_ms AS updated_at_unix_ms,revision AS updated_at FROM aml_sync_state_current WHERE initialized=1)"##);
    }
    if names.contains("eth_protocols") {
        items.push(r##"eth_protocols AS (SELECT network_id,subject_id AS contract_address,protocol,protocol_type,contract_role,ifNull(remote_network_id,'') AS remote_network_id,
 ifNull(remote_subject_id,'') AS remote_contract_address,decoder,source_id AS source,toFloat32(confidence) AS confidence,enabled,created_at_unix_ms FROM aml_protocol_registry_active)"##);
    }
    if names.contains("eth_labels") {
        items.push(r##"eth_labels AS (SELECT * EXCEPT(source_record_id,review_status),claim_id AS label_id,ifNull(source_record_id,'') AS source_record_id,upper(review_status) AS review_status FROM aml_intelligence_claims_current)"##);
    }
    if names.contains("eth_entities") {
        items.push(r##"eth_entities AS (SELECT c.network_id AS network_id,c.address AS address,c.entity_id AS entity_id,c.entity_name AS entity_name,c.entity_type AS entity_type,
 upper(c.address_role) AS address_role,toFloat32(c.confidence) AS confidence,c.risk_level AS risk_level,c.is_exposure_seed AS is_exposure_seed,c.seed_category AS seed_category,
 c.claim_id AS source_label_id,c.source_id AS source_id,c.reviewed_by AS reviewed_by,c.revision AS claim_revision,c.evidence_refs AS evidence_refs,
 c.created_at_unix_ms AS created_at_unix_ms FROM aml_entity_addresses_active AS a INNER JOIN aml_intelligence_claims_active AS c ON a.network_id=c.network_id AND a.claim_id=c.claim_id)"##);
    }
    if names.contains("eth_memberships") {
        items.push(r##"eth_memberships AS (SELECT * EXCEPT(confidence,entity_id),subject_id AS address,ifNull(m.entity_id,'') AS entity_id,toFloat32(ifNull(m.confidence,0)) AS confidence,claim_id AS source_claim_id,toUInt8(status='active') AS is_active FROM aml_cluster_memberships_visible AS m WHERE status='active' )"##);
    }

    if names.contains("eth_exposure_runs") {
        items.push(r##"eth_exposure_runs AS (SELECT * EXCEPT(status),if(r.status='complete',if(seed_count=0,'NO_SEEDS','COMPLETE'),upper(r.status)) AS status FROM aml_exposure_runs_current AS r)"##);
    }
    if names.contains("eth_exposure_paths") {
        items.push(r##"eth_exposure_paths AS (SELECT p.network_id AS network_id,p.path_id AS path_id,p.run_id AS run_id,p.subject_id AS subject_address,s.address AS seed_address,
 ifNull(p.seed_entity_id,'') AS seed_entity_id,ifNull(p.seed_category,'') AS seed_category,p.direction AS direction,p.hop_count AS hop_count,
 if(p.asset_id='native:ETH',concat(p.network_id,'/native:eth'),concat(p.network_id,'/',p.asset_id)) AS asset_id,
 p.amount_share AS amount_share,ifNull(p.time_weight,0) AS time_weight,p.exposure_score AS exposure_score,ifNull(p.service_mediated,toUInt8(0)) AS service_mediated,
 ifNull(p.continuity_type,'observed_path') AS continuity_type,p.path_subjects AS path_addresses,p.relationship_ids AS relationship_ids,p.tx_hashes AS tx_hashes,
 p.evidence_refs AS evidence_refs,p.created_at_unix_ms AS created_at_unix_ms FROM aml_exposure_paths_visible AS p INNER JOIN (SELECT s.* FROM aml_exposure_seeds_current AS s INNER JOIN aml_intelligence_claims_active AS c
 ON s.network_id=c.network_id AND s.claim_id=c.claim_id AND s.address=c.address AND s.claim_revision=c.revision
 WHERE s.is_active=1 AND c.is_exposure_seed=1 AND c.risk_level>0) AS s ON p.network_id=s.network_id AND p.seed_id=s.seed_id)"##);
    }

    if items.is_empty() {
        sql.to_owned()
    } else {
        format!("WITH {} {}", items.join(","), sql)
    }
}
