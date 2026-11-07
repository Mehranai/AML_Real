//! Query-local projections over the common tables; no compatibility DDL.
pub fn expand(sql: &str) -> String {
    let words = sql
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .collect::<std::collections::BTreeSet<_>>();
    let definitions = [
        (
            "bsc_sync",
            "SELECT *,last_block_number+1 AS next_block,last_block_number AS last_finalized_block,last_block_hash AS last_finalized_block_hash,revision AS state_revision FROM aml_sync_state_current WHERE initialized=1",
        ),
        (
            "bsc_transactions",
            r#"SELECT t.* EXCEPT(from_address,to_address,status),ifNull(t.from_address,'') AS from_address,ifNull(t.to_address,'') AS to_address,
 JSONExtractString(raw_evidence_json,'contract_address') AS contract_address,JSONExtractUInt(raw_evidence_json,'nonce') AS nonce,
 JSONExtractUInt(raw_evidence_json,'transaction_type') AS transaction_type,ifNull(amount_raw,toUInt256(0)) AS value,
 JSONExtractString(raw_evidence_json,'input_selector') AS input_selector,JSONExtractString(raw_evidence_json,'input_data') AS input_data,
 toUInt8(t.status='success') AS status,toUInt8(t.status IN ('success','failed')) AS status_known,
 JSONExtractUInt(raw_evidence_json,'gas_limit') AS gas_limit,JSONExtractUInt(raw_evidence_json,'gas_used') AS gas_used,
 fee_raw AS fee_paid,revision AS block_state_revision FROM aml_transactions_canonical AS t"#,
        ),
        (
            "bsc_relationships",
            r#"SELECT r.* EXCEPT(from_address,to_address,asset_id,token_id),ifNull(r.from_address,'') AS from_address,ifNull(r.to_address,'') AS to_address,
 if(r.asset_id='native:BNB','eip155:56/native:bnb',concat('eip155:56/',r.asset_id)) AS asset_id,ifNull(toString(r.token_id),'') AS token_id,
 r.amount_raw AS amount,r.revision AS block_state_revision FROM aml_address_relationships_canonical AS r"#,
        ),
        (
            "bsc_blocks",
            r#"SELECT *,revision AS current_revision,toUInt32(JSONExtractUInt(expected_counts_json,'aml_evm_logs')) AS log_count,
 ifNull(receipt_coverage,toUInt8(0)) AS receipt_data_complete,ifNull(trace_coverage,toUInt8(0)) AS trace_data_complete,
 source_id AS rpc_provider,JSONExtractString(raw_evidence_json,'rpc_client_version') AS rpc_client_version,
 created_at_unix_ms AS indexed_at_unix_ms FROM aml_ingested_blocks_canonical"#,
        ),
        (
            "bsc_events",
            r#"SELECT e.* EXCEPT(asset_in,asset_out,amount_in,amount_out,remote_network_id,confidence),subject_id AS subject_address,
 JSONExtractString(raw_evidence_json,'protocol_contract') AS protocol_contract,JSONExtractString(raw_evidence_json,'counterparty_address') AS counterparty_address,
 JSONExtractString(raw_evidence_json,'asset_in') AS asset_in,JSONExtractString(raw_evidence_json,'asset_out') AS asset_out,
 ifNull(e.remote_network_id,'') AS remote_network_id,JSONExtractString(raw_evidence_json,'remote_receiver') AS remote_receiver,
 JSONExtractString(raw_evidence_json,'bridge_message_id') AS bridge_message_id,JSONExtractString(raw_evidence_json,'bridge_direction') AS bridge_direction,
 JSONExtractString(raw_evidence_json,'amount_in') AS amount_in,JSONExtractString(raw_evidence_json,'amount_out') AS amount_out,toFloat32(e.confidence) AS confidence
 FROM aml_semantic_aml_events_canonical AS e"#,
        ),
        (
            "bsc_discoveries",
            r#"SELECT *,arrayElement(splitByChar(':',asset_id),-1) AS token_address,block_number AS discovered_block,revision AS block_state_revision FROM aml_metadata_discoveries_canonical"#,
        ),
        (
            "bsc_metadata",
            r#"SELECT *,arrayElement(splitByChar(':',asset_id),-1) AS token_address,asset_standard AS token_standard,source_id AS metadata_source FROM aml_asset_metadata_visible"#,
        ),
        (
            "bsc_metadata_jobs",
            r#"SELECT *,arrayElement(splitByChar(':',asset_id),-1) AS token_address,attempts AS attempt_count,next_attempt_unix_ms AS next_attempt_at_unix_ms FROM aml_metadata_jobs_visible"#,
        ),
        (
            "bsc_intelligence",
            "SELECT c.* EXCEPT(is_exposure_seed),toBool(c.is_exposure_seed) AS is_exposure_seed FROM aml_intelligence_claims_active AS c",
        ),
        (
            "bsc_protocols",
            r#"SELECT network_id,subject_id AS contract_address,protocol,protocol_type,contract_role,decoder,
 ifNull(remote_network_id,'') AS remote_network_id,ifNull(remote_subject_id,'') AS remote_contract_address,
 JSONExtract(decoder_config_json,'method_ids','Array(String)') AS method_ids,JSONExtract(decoder_config_json,'method_event_types','Array(String)') AS method_event_types,
 JSONExtract(decoder_config_json,'event_topics','Array(String)') AS event_topics,JSONExtract(decoder_config_json,'event_types','Array(String)') AS event_types,
 toInt8(JSONExtractInt(decoder_config_json,'remote_receiver_topic_index')) AS remote_receiver_topic_index,toInt8(JSONExtractInt(decoder_config_json,'message_topic_index')) AS message_topic_index,
 source_id,source_reference,review_status,toFloat32(confidence) AS evidence_confidence,enabled,revision AS registry_revision,reviewed_by,review_note,created_at_unix_ms
 FROM aml_protocol_registry_active"#,
        ),
    ];
    let parts = definitions
        .into_iter()
        .filter(|(name, _)| words.contains(name))
        .map(|(name, body)| format!("{name} AS ({body})"))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        sql.to_owned()
    } else {
        format!("WITH {} {sql}", parts.join(","))
    }
}
