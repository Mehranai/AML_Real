//! Transitional DTO projections, not database tables or a second writer.
//! All projections read the shared canonical/active/visible contract.
use std::collections::BTreeSet;

fn projection(name: &str) -> Option<String> {
    Some(match name {
        "transactions"|"transactions_canonical"=> r#"SELECT tx_hash,block_number,block_timestamp_unix_ms AS timestamp,
        JSONExtractString(raw_evidence_json,'initiator_address') AS initiator_address,
        JSONExtractString(raw_evidence_json,'target_address') AS target_address,
        JSONExtractString(raw_evidence_json,'contract_address') AS contract_address,
        JSONExtractString(raw_evidence_json,'contract_type') AS contract_type,
        fee_raw AS fee,JSONExtractUInt(raw_evidence_json,'energy_usage_total') AS energy_usage_total,
        JSONExtractUInt(raw_evidence_json,'net_usage') AS net_usage,toUInt8(tx.status='success') AS status,
        toUInt8(tx.status IN ('success','failed')) AS status_known,transaction_index,block_hash,revision
        FROM aml_transactions_canonical AS tx WHERE network_id='tron:mainnet'"#.into(),
        "address_relationships"|"address_relationships_canonical"=>r#"SELECT r.relationship_id AS relationship_id,ifNull(r.from_address,'') AS from_address,ifNull(r.to_address,'') AS to_address,
        JSONExtractString(r.raw_evidence_json,'legacy_asset_id') AS token_address,
        r.tx_hash AS tx_hash,r.block_number AS block_number,r.block_timestamp_unix_ms AS timestamp,r.amount_raw AS amount,r.transfer_type AS transfer_type,
        r.transaction_index AS transaction_index,r.event_index AS event_index,r.event_sub_index AS event_sub_index,r.token_id AS token_id,r.asset_id AS asset_id,r.block_hash AS block_hash,r.revision AS revision,
        multiIf(JSONExtractUInt(f.features_json,'is_swap')=1,'swap',JSONExtractUInt(f.features_json,'is_bridge')=1,'bridge',JSONExtractUInt(f.features_json,'is_liquidity_add')=1,'liquidity_add',JSONExtractUInt(f.features_json,'is_liquidity_remove')=1,'liquidity_remove',JSONExtractUInt(f.features_json,'is_mint')=1,'mint',JSONExtractUInt(f.features_json,'is_burn')=1,'burn',JSONExtractString(f.features_json,'transaction_type') NOT IN ('','unknown','exchange_flow'),JSONExtractString(f.features_json,'transaction_type'),r.transfer_type) AS operation_type,
        JSONExtractString(f.features_json,'protocol') AS protocol,JSONExtractString(f.features_json,'transaction_type') AS transaction_type,JSONExtractString(f.features_json,'transaction_subtype') AS transaction_subtype,
        toFloat32(JSONExtractFloat(f.features_json,'classification_confidence')) AS classification_confidence,JSONExtractString(f.features_json,'classification_source') AS classification_source,
        JSONExtractString(f.features_json,'method_id') AS method_id,toUInt8(JSONExtractUInt(f.features_json,'is_contract_call')) AS is_contract_call,fromUnixTimestamp64Milli(toInt64(r.created_at_unix_ms)) AS inserted_at
        FROM aml_address_relationships_canonical AS r
        LEFT JOIN aml_transaction_features_canonical AS f ON r.network_id=f.network_id AND r.block_number=f.block_number AND r.block_hash=f.block_hash AND r.commit_id=f.commit_id AND r.revision=f.revision AND r.tx_hash=f.tx_hash
        WHERE r.network_id='tron:mainnet'"#.into(),
        "transaction_features"=>{
            let strings=["transaction_type","transaction_subtype","classification_source","protocol","method_id"].map(|k|format!("JSONExtractString(features_json,'{k}') AS {k}"));
            let flags=["is_swap","is_bridge","is_mint","is_burn","is_liquidity_add","is_liquidity_remove","is_contract_call"].map(|k|format!("toUInt8(JSONExtractUInt(features_json,'{k}')) AS {k}"));
            format!("SELECT tx_hash,block_number,block_timestamp_unix_ms AS timestamp,{},toFloat32(JSONExtractFloat(features_json,'classification_confidence')) AS classification_confidence,{},fromUnixTimestamp64Milli(toInt64(created_at_unix_ms)) AS inserted_at FROM aml_transaction_features_canonical WHERE network_id='tron:mainnet'",strings.join(","),flags.join(","))
        },
        "semantic_aml_events"|"semantic_aml_events_canonical"=>r#"SELECT event_id,'tron' AS chain,tx_hash,block_number,block_timestamp_unix_ms AS timestamp,
        event_type,subject_id AS subject_address,protocol,JSONExtractString(raw_evidence_json,'asset_in') AS asset_in,
        JSONExtractString(raw_evidence_json,'asset_out') AS asset_out,detector,detector_version,toFloat32(confidence) AS confidence,evidence_json
        FROM aml_semantic_aml_events_canonical WHERE network_id='tron:mainnet'"#.into(),
        "ingested_blocks"=>r#"SELECT 'tron' AS chain,block_number,block_hash,parent_hash,block_timestamp_unix_ms AS block_timestamp,transaction_count,
        'NOT_ASSERTED' AS finality_status,upper(ingestion_status) AS ingestion_status,'' AS error_message,created_at_unix_ms AS indexed_at_unix_ms,
        revision,receipt_coverage,trace_coverage FROM aml_ingested_blocks_canonical WHERE network_id='tron:mainnet'"#.into(),
        "sync_state"=>r#"SELECT 'tron' AS chain,last_block_number AS last_synced_block,revision AS updated_at FROM aml_sync_state_current WHERE network_id='tron:mainnet' AND initialized=1"#.into(),
        "token_metadata_discoveries"=>r#"SELECT asset_id AS canonical_asset_id,
        JSONExtractString(raw_evidence_json,'token_address') AS token_address,
        block_number AS discovered_block,created_at_unix_ms AS discovered_at_unix_ms
        FROM aml_metadata_discoveries_canonical WHERE network_id='tron:mainnet' AND standard_hint='trc20'"#.into(),
        "ingestion_failures"=>r#"SELECT failure_id,'tron' AS chain,block_number,ifNull(block_hash,'') AS block_hash,ifNull(tx_hash,'') AS tx_hash,
        operation AS stage,error_code AS error_class,error_message,ifNull(retryable,toUInt8(0)) AS retryable,attempts AS attempt_count,if(f.status!='resolved','OPEN','RESOLVED') AS status,
        ifNull(first_failed_at_unix_ms,created_at_unix_ms) AS first_failed_at_unix_ms,
        ifNull(last_failed_at_unix_ms,created_at_unix_ms) AS last_failed_at_unix_ms,
        ifNull(resolved_at_unix_ms,toUInt64(0)) AS resolved_at_unix_ms
        FROM aml_ingestion_failures_current AS f WHERE network_id='tron:mainnet'"#.into(),
        "exchange_flows_canonical"=>r#"SELECT lower(hex(SHA256(concat(r.relationship_id,'|',ifNull(f.entity_id,''),'|',ifNull(t.entity_id,''))))) AS flow_id,
        r.tx_hash AS tx_hash,r.block_number AS block_number,r.from_address AS from_address,r.to_address AS to_address,
        multiIf(f.address!='' AND t.address!='' AND f.entity_id!=t.entity_id,concat(f.exchange_name,' -> ',t.exchange_name),t.address!='',t.exchange_name,f.exchange_name) AS exchange_name,
        multiIf(f.address!='' AND t.address!='' AND f.entity_id=t.entity_id AND upper(f.address_role)='DEPOSIT' AND upper(t.address_role) IN ('HOT','COLD'),'sweep',f.address!='' AND t.address!='','exchange_transfer',t.address!='','deposit','withdrawal') AS flow_type,
        r.token_address AS token_address,r.amount AS amount,toFloat32(multiIf(f.address!='' AND t.address!='',least(f.confidence,t.confidence),t.address!='',t.confidence,f.confidence)) AS confidence
        FROM tron_address_relationships_canonical AS r
        LEFT JOIN (SELECT * FROM tron_exchange_addresses WHERE is_active=1) AS f ON f.address=r.from_address
        LEFT JOIN (SELECT * FROM tron_exchange_addresses WHERE is_active=1) AS t ON t.address=r.to_address
        WHERE f.address!='' OR t.address!=''"#.into(),
        "intelligence_sources"=>"SELECT *, 'tron' AS chain FROM aml_intelligence_sources_current WHERE network_id='tron:mainnet'".into(),
        "entity_labels"=>r#"SELECT claim_id AS label_id,'tron' AS chain,address,entity_id,entity_name,entity_type,upper(address_role) AS address_role,
        toFloat32(confidence) AS confidence,risk_level AS risk_percent,source_id AS source,ifNull(source_record_id,'') AS source_record_id,
        ifNull(supersedes_claim_id,'') AS supersedes_label_id,ifNull(submitted_by,'') AS submitted_by,ifNull(case_id,'') AS case_id,evidence_refs,
        upper(review_status) AS review_status,created_at_unix_ms FROM aml_intelligence_claims_current WHERE network_id='tron:mainnet' AND claim_kind='label'"#.into(),
        "intelligence_reviews"=>r#"SELECT review_id,'tron' AS chain,JSONExtractString(raw_evidence_json,'subject_type') AS subject_type,subject_id,
        upper(decision) AS decision,reviewed_by AS reviewer,reason,evidence_refs,created_at_unix_ms,revision,subject_revision
        FROM aml_intelligence_reviews_current AS r WHERE network_id='tron:mainnet' AND subject_revision IN
        (SELECT revision FROM aml_intelligence_claims_current WHERE network_id='tron:mainnet')"#.into(),
        "address_cluster_claims"=>r#"SELECT claim_id,'tron' AS chain,address,entity_id AS cluster_id,entity_type AS cluster_type,upper(address_role) AS address_role,
        JSONExtractString(raw_evidence_json,'claim_method') AS claim_method,toFloat32(confidence) AS confidence,source_id AS source,ifNull(source_record_id,'') AS source_record_id,
        JSONExtract(raw_evidence_json,'evidence_tx_hashes','Array(String)') AS evidence_tx_hashes,JSONExtract(raw_evidence_json,'evidence_addresses','Array(String)') AS evidence_addresses,
        evidence_json,ifNull(supersedes_claim_id,'') AS supersedes_claim_id,upper(review_status) AS review_status,ifNull(submitted_by,'') AS created_by,created_at_unix_ms
        FROM aml_intelligence_claims_current WHERE network_id='tron:mainnet' AND claim_kind='cluster'"#.into(),
        "address_entity"=>r#"SELECT c.address AS address,c.entity_id AS entity_id,c.entity_name AS entity_name,c.entity_type AS entity_type,
        toFloat32(c.confidence) AS confidence,c.claim_id AS source,toUInt8(1) AS is_active,fromUnixTimestamp64Milli(toInt64(c.created_at_unix_ms)) AS created_at,
        c.revision AS revision,c.evidence_refs AS evidence_refs,c.source_id AS source_id FROM aml_entity_addresses_active AS a
        INNER JOIN aml_intelligence_claims_active AS c ON a.network_id=c.network_id AND a.claim_id=c.claim_id AND a.address=c.address
        WHERE a.network_id='tron:mainnet' AND c.claim_kind='label'"#.into(),
        "exchange_addresses"=>r#"SELECT c.address AS address,c.entity_id AS entity_id,c.entity_name AS exchange_name,upper(c.address_role) AS address_role,
        toFloat32(c.confidence) AS confidence,concat('entity_label:',c.claim_id) AS detection_source,toUInt8(1) AS is_active,
        a.first_seen_block AS first_seen_block,a.last_seen_block AS last_seen_block,c.revision AS revision
        FROM aml_entity_addresses_active AS a INNER JOIN aml_intelligence_claims_active AS c ON a.network_id=c.network_id AND a.claim_id=c.claim_id AND a.address=c.address
        WHERE a.network_id='tron:mainnet' AND c.claim_kind='label' AND lower(c.entity_type) IN ('exchange','centralized_exchange','custodial_exchange','dex')"#.into(),
        "exposure_seeds"=>r#"SELECT s.seed_id AS seed_id,s.address AS address,c.entity_name AS entity_name,c.entity_type AS entity_type,c.risk_level AS risk_level,
        c.source_id AS source,c.claim_id AS source_label_id,toUInt8(1) AS is_active,c.revision AS claim_revision,s.revision AS revision
        FROM aml_exposure_seeds_current AS s INNER JOIN aml_intelligence_claims_active AS c ON s.network_id=c.network_id AND s.claim_id=c.claim_id AND s.address=c.address AND s.claim_revision=c.revision
        WHERE s.network_id='tron:mainnet' AND s.is_active=1 AND c.claim_kind='label' AND c.is_exposure_seed=1 AND c.risk_level>0"#.into(),
        "address_cluster_memberships"=>r#"SELECT 'tron' AS chain,subject_id AS address,cluster_id,cluster_type,upper(address_role) AS address_role,
        toFloat32(ifNull(confidence,0)) AS confidence,JSONExtractString(evidence_json,'intelligence_claim_id') AS source_claim_id,cluster_version,
        toUInt8(status='active') AS is_active,created_at_unix_ms,revision FROM aml_cluster_memberships_visible WHERE network_id='tron:mainnet'"#.into(),
        "cluster_versions"=>"SELECT *, 'tron' AS chain,cluster_version AS version FROM aml_cluster_versions_visible WHERE network_id='tron:mainnet'".into(),
        "token_metadata"=>r#"SELECT asset_id,symbol,name,decimals,ifNull(is_verified,toUInt8(0)) AS is_verified,
        if(startsWith(asset_id,'trc20:'),JSONExtractString(raw_evidence_json,'token_address'),'') AS token_address,
        updated_at_unix_ms,revision FROM aml_asset_metadata_visible WHERE network_id='tron:mainnet' AND metadata_status='complete'"#.into(),
        "address_exposure"=>r#"SELECT source_subject_id AS source_address,subject_id AS exposed_address,hop_distance,ifNull(exposure_score,0) AS exposure_score,
        ifNull(path_count,toUInt64(0)) AS path_count,ifNull(last_tx_hash,'') AS last_tx_hash,ifNull(last_seen_block,toUInt64(0)) AS last_seen_block,exposure_type,
        ifNull(best_path_amount_share,0) AS best_path_amount_share,ifNull(best_path_time_weight,0) AS best_path_time_weight,
        ifNull(service_mediated,toUInt8(0)) AS service_mediated,run_id AS propagation_run_id FROM aml_address_exposures_visible WHERE network_id='tron:mainnet'"#.into(),
        "exposure_runs"=>r#"SELECT JSONExtractString(raw_evidence_json,'source_address') AS source_address,run_id AS propagation_run_id,upper(status) AS status,max_hops,
        JSONExtractUInt(raw_evidence_json,'row_count') AS row_count,ifNull(completed_at_unix_ms,toUInt64(0)) AS completed_at_unix_ms FROM aml_exposure_runs_visible WHERE network_id='tron:mainnet'"#.into(),
        _=>return None,
    })
}

// Lex only identifiers outside quoted strings/comments. Bind placeholders and
// caller values stay untouched; FINAL is removed only on an adapted relation.
pub fn expand(sql: &str) -> String {
    let trimmed = sql.trim_start();
    if !trimmed.to_ascii_uppercase().starts_with("SELECT")
        && !trimmed.to_ascii_uppercase().starts_with("WITH")
    {
        return sql.into();
    }
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::<(usize, usize, String)>::new();
    while i < bytes.len() {
        if matches!(bytes[i], b'\'' | b'"' | b'`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    if bytes.get(i) == Some(&quote) {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if bytes[i..].starts_with(b"--") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((start, i, sql[start..i].into()));
        } else {
            i += 1;
        }
    }
    let mut names = BTreeSet::new();
    let mut changes = Vec::new();
    for pair in tokens.windows(2) {
        if !matches!(pair[0].2.to_ascii_uppercase().as_str(), "FROM" | "JOIN") {
            continue;
        }
        let (start, end, name) = &pair[1];
        if !sql[pair[0].1..*start].trim().is_empty() || projection(name).is_none() {
            continue;
        }
        names.insert(name.clone());
        changes.push((*start, *end, format!("tron_{name}")));
        // Optional alias can precede FINAL (FROM t AS x FINAL).
        if let Some(index) = tokens.iter().position(|t| t.0 == *start) {
            let mut n = index + 1;
            if tokens
                .get(n)
                .is_some_and(|t| t.2.eq_ignore_ascii_case("AS"))
            {
                n += 2;
            }
            if let Some(t) = tokens.get(n).filter(|t| t.2.eq_ignore_ascii_case("FINAL")) {
                changes.push((t.0, t.1, String::new()));
            }
        }
    }
    if names.is_empty() {
        return sql.into();
    }
    if names.contains("exchange_flows_canonical") {
        names.insert("address_relationships_canonical".into());
        names.insert("exchange_addresses".into());
    }
    let items = names
        .iter()
        .map(|n| format!("tron_{n} AS ({})", projection(n).unwrap()))
        .collect::<Vec<_>>();
    let mut rendered = sql.to_owned();
    changes.sort_by_key(|c| c.0);
    for (a, b, value) in changes.into_iter().rev() {
        rendered.replace_range(a..b, &value);
    }
    if rendered
        .trim_start()
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("WITH"))
    {
        let start = rendered.len() - rendered.trim_start().len() + 4;
        format!(
            "{} {},{}",
            &rendered[..start],
            items.join(","),
            &rendered[start..]
        )
    } else {
        format!("WITH {} {rendered}", items.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapts_relations_without_touching_values_and_keeps_other_final_reads() {
        let sql = expand(
            "SELECT 'FROM transactions FINAL ?' AS note,t.tx_hash FROM transactions AS t FINAL JOIN untouched_lookup AS e FINAL ON 1=1 WHERE t.tx_hash=?",
        );
        assert!(sql.contains("'FROM transactions FINAL ?'"));
        assert!(sql.contains("FROM tron_transactions AS t  JOIN untouched_lookup AS e FINAL"));
        assert_eq!(sql.matches('?').count(), 2);
        assert!(
            expand("WITH\n x AS (SELECT * FROM transactions) SELECT * FROM x")
                .contains("tron_transactions AS")
        );
        assert!(
            !expand("WITH\n x AS (SELECT * FROM transactions) SELECT * FROM x").contains(" WITH\n")
        );
        assert_eq!(
            expand("CREATE VIEW x AS SELECT * FROM transactions"),
            "CREATE VIEW x AS SELECT * FROM transactions"
        );
    }
}
