-- Generated analytics/specialized upgrade; do not edit.
-- Migration: 20261027_0004_analytics_specialized
-- Checksum: 955e0c5ff2e6bb7000793e66408312f4d0172d026f15f328a9431ee050b568ec

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `total_supply_raw` Nullable(UInt256) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `code_hash` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `is_verified` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `metadata_status` LowCardinality(String) DEFAULT 'unknown';

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `observed_block` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `verified_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `reviewed_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `updated_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_asset_metadata ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `discovery_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `discovered_block` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `source_revision` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `last_error_class` String DEFAULT '';

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `started_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `completed_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `updated_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_metadata_jobs ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_intelligence_sources ADD COLUMN IF NOT EXISTS `metadata_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_intelligence_sources ADD COLUMN IF NOT EXISTS `updated_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_sources ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `source_record_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `supersedes_claim_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `submitted_by` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `case_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `reviewed_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_intelligence_claims ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_intelligence_reviews ADD COLUMN IF NOT EXISTS `subject_revision` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_reviews ADD COLUMN IF NOT EXISTS `supersedes_review_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_reviews ADD COLUMN IF NOT EXISTS `case_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_intelligence_reviews ADD COLUMN IF NOT EXISTS `evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_intelligence_reviews ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `left_subject_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `right_subject_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `same_entity` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `run_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `source_record_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `supersedes_claim_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `created_by` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `reviewed_by` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `review_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `cluster_type` String DEFAULT '';

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `address_role` String DEFAULT '';

ALTER TABLE {{database}}.aml_cluster_claims ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `membership_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `run_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `entity_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `membership_type` String DEFAULT '';

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `confidence` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `review_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `cluster_type` String DEFAULT '';

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `address_role` String DEFAULT '';

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_cluster_memberships ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_cluster_runs ADD COLUMN IF NOT EXISTS `entity_memberships` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_runs ADD COLUMN IF NOT EXISTS `control_claims` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_cluster_runs ADD COLUMN IF NOT EXISTS `error_message` String DEFAULT '';

ALTER TABLE {{database}}.aml_cluster_runs ADD COLUMN IF NOT EXISTS `settings_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_cluster_runs ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `entity_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `entity_name` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `entity_type` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `risk_level` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `source_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `claim_revision` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `evidence_refs` Array(String) DEFAULT [];

ALTER TABLE {{database}}.aml_exposure_seeds ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `seed_ids` Array(String) DEFAULT [];

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `seed_snapshot_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `detector_version` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `hop_decay` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `time_half_life_days` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `max_paths_per_subject` Nullable(UInt32) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `seed_count` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `path_count` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `settings_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `error_message` String DEFAULT '';

ALTER TABLE {{database}}.aml_exposure_runs ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `seed_entity_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `seed_category` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `seed_risk_level` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `time_weight` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `service_mediated` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `service_entity_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `continuity_type` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `first_transfer_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `last_transfer_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `detector_version` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `allocation_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_exposure_paths ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_risk_policies ADD COLUMN IF NOT EXISTS `settings_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_risk_policies ADD COLUMN IF NOT EXISTS `source_id` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_risk_policies ADD COLUMN IF NOT EXISTS `updated_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_risk_policies ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_risk_policy_rules ADD COLUMN IF NOT EXISTS `settings_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_risk_policy_rules ADD COLUMN IF NOT EXISTS `updated_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_risk_policy_rules ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_wallet_risk_signals ADD COLUMN IF NOT EXISTS `signal_strength` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_wallet_risk_signals ADD COLUMN IF NOT EXISTS `as_of_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_wallet_risk_signals ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD COLUMN IF NOT EXISTS `top_reasons` Array(String) DEFAULT [];

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD COLUMN IF NOT EXISTS `evidence_refs` Array(String) DEFAULT [];

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD COLUMN IF NOT EXISTS `as_of_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD COLUMN IF NOT EXISTS `scoring_method` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD COLUMN IF NOT EXISTS `evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_evm_logs ADD COLUMN IF NOT EXISTS `transaction_index` Nullable(UInt32) DEFAULT NULL;

ALTER TABLE {{database}}.aml_evm_logs ADD COLUMN IF NOT EXISTS `removed` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_evm_logs ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

CREATE TABLE IF NOT EXISTS {{database}}.aml_metadata_discoveries (`network_id` LowCardinality(String),`block_number` UInt64,`block_hash` String,`block_timestamp_unix_ms` UInt64,`commit_id` String,`revision` UInt64,`discovery_id` String DEFAULT '',`tx_hash` String DEFAULT '',`transaction_index` UInt32 DEFAULT 0,`asset_id` String DEFAULT '',`standard_hint` String DEFAULT '',`evidence_id` String DEFAULT '',`source_id` String DEFAULT '',`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND ((network_id='tron:mainnet' AND match(`block_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`block_hash`,'^[0-9a-f]{64}$'))) AND ((network_id='tron:mainnet' AND match(`tx_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`tx_hash`,'^[0-9a-f]{64}$'))) AND (((network_id='tron:mainnet' AND match(`asset_id`,'^(?:native:TRX|trc10:(?:0|[1-9][0-9]*)|trc20:41[0-9a-f]{40}|trc721:41[0-9a-f]{40}(?:/(?:0|[1-9][0-9]*))?)$')) OR (network_id='eip155:1' AND match(`asset_id`,'^(?:native:ETH|erc20:0x[0-9a-f]{40}|(?:erc721|erc1155):0x[0-9a-f]{40}(?:/(?:0|[1-9][0-9]*))?)$')) OR (network_id='eip155:56' AND match(`asset_id`,'^(?:native:BNB|erc20:0x[0-9a-f]{40}|(?:erc721|erc1155):0x[0-9a-f]{40}(?:/(?:0|[1-9][0-9]*))?)$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`asset_id`,'^(?:native:BTC)$'))) AND (length(extract(`asset_id`,'([0-9]+)$'))<78 OR (length(extract(`asset_id`,'([0-9]+)$'))=78 AND extract(`asset_id`,'([0-9]+)$')<='115792089237316195423570985008687907853269984665640564039457584007913129639935'))) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`block_hash`)>0) AND (length(`commit_id`)>0) AND (length(`discovery_id`)>0)) ENGINE = ReplacingMergeTree(revision) PARTITION BY intDiv(block_number,100000) ORDER BY (network_id,block_number,block_hash,commit_id,discovery_id) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_protocol_registry (`network_id` LowCardinality(String),`registry_id` String DEFAULT '',`subject_id` String DEFAULT '',`subject_type` LowCardinality(String) DEFAULT 'address',`protocol` String DEFAULT '',`protocol_type` String DEFAULT '',`contract_role` String DEFAULT '',`remote_network_id` Nullable(String) DEFAULT NULL,`remote_subject_id` Nullable(String) DEFAULT NULL,`decoder` String DEFAULT '',`decoder_config_json` String DEFAULT '{}',`source_id` String DEFAULT '',`source_reference` String DEFAULT '',`confidence` Float64 DEFAULT 0,`enabled` UInt8 DEFAULT 0,`review_status` LowCardinality(String) DEFAULT 'pending',`reviewed_by` String DEFAULT '',`reviewed_at_unix_ms` Nullable(UInt64) DEFAULT NULL,`review_note` String DEFAULT '',`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`revision` UInt64,`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND (isValidJSON(`decoder_config_json`)) AND (isFinite(`confidence`) AND `confidence`>=0 AND `confidence`<=1) AND (`enabled` IN (0,1)) AND (`review_status` IN ('pending','approved','rejected','revoked')) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`registry_id`)>0) AND (review_status='pending' OR length(`reviewed_by`)>0) AND (`subject_type` IN ('address','script','contract'))) ENGINE = ReplacingMergeTree(revision) ORDER BY (network_id,registry_id) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_entities (`network_id` LowCardinality(String),`entity_id` String DEFAULT '',`entity_name` String DEFAULT '',`entity_type` String DEFAULT '',`source_id` String DEFAULT '',`source_reference` String DEFAULT '',`is_active` UInt8 DEFAULT 0,`review_status` LowCardinality(String) DEFAULT 'pending',`reviewed_by` String DEFAULT '',`reviewed_at_unix_ms` Nullable(UInt64) DEFAULT NULL,`review_note` String DEFAULT '',`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`revision` UInt64,`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND (`is_active` IN (0,1)) AND (`review_status` IN ('pending','approved','rejected','revoked')) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`entity_id`)>0) AND (review_status='pending' OR length(`reviewed_by`)>0)) ENGINE = ReplacingMergeTree(revision) ORDER BY (network_id,entity_id) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_entity_addresses (`network_id` LowCardinality(String),`entity_id` String DEFAULT '',`address` String DEFAULT '',`address_role` String DEFAULT '',`claim_id` String DEFAULT '',`source_id` String DEFAULT '',`confidence` Float64 DEFAULT 0,`is_active` UInt8 DEFAULT 0,`first_seen_block` Nullable(UInt64) DEFAULT NULL,`last_seen_block` Nullable(UInt64) DEFAULT NULL,`review_status` LowCardinality(String) DEFAULT 'pending',`reviewed_by` String DEFAULT '',`reviewed_at_unix_ms` Nullable(UInt64) DEFAULT NULL,`review_note` String DEFAULT '',`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`revision` UInt64,`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND (isFinite(`confidence`) AND `confidence`>=0 AND `confidence`<=1) AND (`is_active` IN (0,1)) AND (`review_status` IN ('pending','approved','rejected','revoked')) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`entity_id`)>0) AND (length(`address`)>0) AND (length(`claim_id`)>0) AND (review_status='pending' OR length(`reviewed_by`)>0)) ENGINE = ReplacingMergeTree(revision) ORDER BY (network_id,entity_id,address,claim_id) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_cluster_versions (`network_id` LowCardinality(String),`cluster_id` String DEFAULT '',`cluster_version` String DEFAULT '',`cluster_type` String DEFAULT '',`display_name` String DEFAULT '',`change_type` String DEFAULT '',`change_reason` String DEFAULT '',`source_claim_ids` Array(String) DEFAULT [],`active_member_count` Nullable(UInt64) DEFAULT NULL,`created_by` String DEFAULT '',`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`revision` UInt64,`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`cluster_id`)>0) AND (length(`cluster_version`)>0)) ENGINE = ReplacingMergeTree(revision) ORDER BY (network_id,cluster_id,cluster_version) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_address_exposures (`network_id` LowCardinality(String),`exposure_id` String DEFAULT '',`run_id` String DEFAULT '',`seed_id` String DEFAULT '',`source_subject_id` String DEFAULT '',`subject_id` String DEFAULT '',`hop_distance` UInt8 DEFAULT 0,`exposure_score` Nullable(Float64) DEFAULT NULL,`path_count` Nullable(UInt64) DEFAULT NULL,`last_tx_hash` Nullable(String) DEFAULT NULL,`last_seen_block` Nullable(UInt64) DEFAULT NULL,`exposure_type` String DEFAULT '',`best_path_amount_share` Nullable(Float64) DEFAULT NULL,`best_path_time_weight` Nullable(Float64) DEFAULT NULL,`service_mediated` Nullable(UInt8) DEFAULT NULL,`as_of_block` Nullable(UInt64) DEFAULT NULL,`source_revision` Nullable(UInt64) DEFAULT NULL,`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`revision` UInt64,`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND (isNull(`exposure_score`) OR (isFinite(`exposure_score`) AND `exposure_score`>=0 AND `exposure_score`<=1)) AND (isNull(`last_tx_hash`) OR ((network_id='tron:mainnet' AND match(`last_tx_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`last_tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`last_tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`last_tx_hash`,'^[0-9a-f]{64}$')))) AND (isNull(`best_path_amount_share`) OR (isFinite(`best_path_amount_share`) AND `best_path_amount_share`>=0 AND `best_path_amount_share`<=1)) AND (isNull(`best_path_time_weight`) OR (isFinite(`best_path_time_weight`) AND `best_path_time_weight`>=0 AND `best_path_time_weight`<=1)) AND (isNull(`service_mediated`) OR (`service_mediated` IN (0,1))) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`run_id`)>0) AND (length(`exposure_id`)>0)) ENGINE = ReplacingMergeTree(revision) ORDER BY (network_id,run_id,exposure_id) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_bitcoin_scripts (`network_id` LowCardinality(String),`block_number` UInt64,`block_hash` String,`block_timestamp_unix_ms` UInt64,`commit_id` String,`revision` UInt64,`tx_hash` String DEFAULT '',`script_hash` String DEFAULT '',`script_hex` String DEFAULT '',`script_type` String DEFAULT '',`address` Nullable(String) DEFAULT NULL,`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND ((network_id='tron:mainnet' AND match(`block_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`block_hash`,'^[0-9a-f]{64}$'))) AND ((network_id='tron:mainnet' AND match(`tx_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`tx_hash`,'^[0-9a-f]{64}$'))) AND ((network_id='tron:mainnet' AND match(`script_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`script_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`script_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`script_hash`,'^[0-9a-f]{64}$'))) AND (match(`script_hex`,'^(?:[0-9a-f]{2})*$')) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`block_hash`)>0) AND (length(`commit_id`)>0) AND (length(`tx_hash`)>0) AND (length(`script_hash`)>0)) ENGINE = ReplacingMergeTree(revision) PARTITION BY intDiv(block_number,100000) ORDER BY (network_id,block_number,block_hash,commit_id,tx_hash,script_hash) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_evm_traces (`network_id` LowCardinality(String),`block_number` UInt64,`block_hash` String,`block_timestamp_unix_ms` UInt64,`commit_id` String,`revision` UInt64,`tx_hash` String DEFAULT '',`trace_id` String DEFAULT '',`transaction_index` UInt32 DEFAULT 0,`trace_address` Array(UInt32) DEFAULT [],`trace_type` String DEFAULT '',`from_address` Nullable(String) DEFAULT NULL,`to_address` Nullable(String) DEFAULT NULL,`value_raw` Nullable(UInt256) DEFAULT NULL,`success` Nullable(UInt8) DEFAULT NULL,`error_message` Nullable(String) DEFAULT NULL,`source_id` String DEFAULT '',`evidence_refs` Array(String) DEFAULT [],`raw_evidence_json` String DEFAULT '{}',`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('eip155:1','eip155:56')) AND (`revision`>0) AND ((network_id='tron:mainnet' AND match(`block_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`block_hash`,'^[0-9a-f]{64}$'))) AND ((network_id='tron:mainnet' AND match(`tx_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`tx_hash`,'^[0-9a-f]{64}$'))) AND (isNull(`success`) OR (`success` IN (0,1))) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`)) AND (length(`block_hash`)>0) AND (length(`commit_id`)>0) AND (length(`tx_hash`)>0) AND (length(`trace_id`)>0)) ENGINE = ReplacingMergeTree(revision) PARTITION BY intDiv(block_number,100000) ORDER BY (network_id,block_number,block_hash,commit_id,tx_hash,trace_id) SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.aml_raw_transactions (`network_id` LowCardinality(String),`block_number` UInt64,`block_hash` String,`block_timestamp_unix_ms` UInt64,`commit_id` String,`revision` UInt64,`tx_hash` String DEFAULT '',`transaction_index` UInt32 DEFAULT 0,`source_id` String DEFAULT '',`payload_json` String DEFAULT '{}',`receipt_json` Nullable(String) DEFAULT NULL,`evidence_refs` Array(String) DEFAULT [],`created_at_unix_ms` UInt64,CONSTRAINT `aml_analytics_v4` CHECK (`network_id` IN ('tron:mainnet','eip155:1','eip155:56','bip122:000000000019d6689c085ae165831e93')) AND (`revision`>0) AND ((network_id='tron:mainnet' AND match(`block_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`block_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`block_hash`,'^[0-9a-f]{64}$'))) AND ((network_id='tron:mainnet' AND match(`tx_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`tx_hash`,'^[0-9a-f]{64}$'))) AND (isValidJSON(`payload_json`)) AND (isNull(`receipt_json`) OR (isValidJSON(`receipt_json`))) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (length(`block_hash`)>0) AND (length(`commit_id`)>0) AND (length(`tx_hash`)>0) AND (network_id!='bip122:000000000019d6689c085ae165831e93' OR isNull(`receipt_json`))) ENGINE = ReplacingMergeTree(revision) PARTITION BY intDiv(block_number,100000) ORDER BY (network_id,block_number,block_hash,commit_id,tx_hash) SETTINGS index_granularity = 8192;

ALTER TABLE {{database}}.aml_asset_metadata ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`code_hash`) OR ((network_id='tron:mainnet' AND match(`code_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`code_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`code_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`code_hash`,'^[0-9a-f]{64}$')))) AND (isNull(`is_verified`) OR (`is_verified` IN (0,1))) AND (isValidJSON(`raw_evidence_json`)) AND (isNull(`total_supply_raw`) OR (network_id!='bip122:000000000019d6689c085ae165831e93' OR `total_supply_raw`<=2100000000000000));

ALTER TABLE {{database}}.aml_metadata_jobs ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_intelligence_sources ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`metadata_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_intelligence_claims ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`evidence_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_intelligence_reviews ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`evidence_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_cluster_claims ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`same_entity`) OR (`same_entity` IN (0,1))) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_cluster_memberships ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`confidence`) OR (isFinite(`confidence`) AND `confidence`>=0 AND `confidence`<=1)) AND (isValidJSON(`evidence_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_cluster_runs ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`settings_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_exposure_seeds ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`risk_level`) OR (isFinite(`risk_level`) AND `risk_level`>=0 AND `risk_level`<=100)) AND (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_exposure_runs ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`seed_snapshot_json`)) AND (isNull(`hop_decay`) OR (isFinite(`hop_decay`) AND `hop_decay`>=0 AND `hop_decay`<=1)) AND (isValidJSON(`settings_json`)) AND (isValidJSON(`raw_evidence_json`)) AND (isNull(`time_half_life_days`) OR (isFinite(`time_half_life_days`) AND `time_half_life_days`>=0));

ALTER TABLE {{database}}.aml_exposure_paths ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`time_weight`) OR (isFinite(`time_weight`) AND `time_weight`>=0 AND `time_weight`<=1)) AND (isNull(`service_mediated`) OR (`service_mediated` IN (0,1))) AND (isValidJSON(`allocation_json`)) AND (isValidJSON(`evidence_json`)) AND (isValidJSON(`raw_evidence_json`)) AND (isNull(`seed_risk_level`) OR (isFinite(`seed_risk_level`) AND `seed_risk_level`>=0 AND `seed_risk_level`<=100));

ALTER TABLE {{database}}.aml_risk_policies ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`settings_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_risk_policy_rules ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isValidJSON(`settings_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_wallet_risk_signals ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`signal_strength`) OR (isFinite(`signal_strength`) AND `signal_strength`>=0 AND `signal_strength`<=1)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_wallet_risk_assessments ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ((network_id='tron:mainnet' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(substring(x,position(x,':')+1),'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(substring(x,position(x,':')+1),'^[0-9a-f]{64}$')))) AND (NOT startsWith(x,'outpoint:') OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(x,'^outpoint:[0-9a-f]{64}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), `evidence_refs`)) AND (isValidJSON(`evidence_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_evm_logs ADD CONSTRAINT IF NOT EXISTS `aml_analytics_v4` CHECK (isNull(`removed`) OR (`removed` IN (0,1))) AND (isValidJSON(`raw_evidence_json`));

CREATE OR REPLACE VIEW {{database}}.aml_sync_state_current AS SELECT * FROM {{database}}.aml_sync_state ORDER BY revision DESC LIMIT 1 BY network_id;

CREATE OR REPLACE VIEW {{database}}.aml_ingested_blocks_canonical AS SELECT network_id, block_number, data.1 AS block_hash, data.2 AS block_timestamp_unix_ms, data.3 AS commit_id, data.4 AS revision, data.5 AS parent_hash, data.6 AS canonical, data.7 AS ingestion_status, data.8 AS transaction_count, data.9 AS receipt_coverage, data.10 AS trace_coverage, data.11 AS prevout_coverage, data.12 AS source_id, data.13 AS created_at_unix_ms, data.14 AS expected_counts_json, data.15 AS raw_evidence_json FROM (SELECT b.network_id AS network_id, b.block_number AS block_number, argMax(tuple(b.block_hash, b.block_timestamp_unix_ms, b.commit_id, b.revision, b.parent_hash, b.canonical, b.ingestion_status, b.transaction_count, b.receipt_coverage, b.trace_coverage, b.prevout_coverage, b.source_id, b.created_at_unix_ms, b.expected_counts_json, b.raw_evidence_json), b.revision) AS data FROM {{database}}.aml_ingested_blocks AS b INNER JOIN {{database}}.aml_sync_state_current AS s ON b.network_id=s.network_id WHERE s.initialized=1 AND b.revision<=s.revision AND b.block_number<=s.last_block_number GROUP BY b.network_id,b.block_number) WHERE data.6=1 AND data.7='complete';

CREATE OR REPLACE VIEW {{database}}.aml_transactions_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.from_address AS from_address, f.to_address AS to_address, f.amount_raw AS amount_raw, f.fee_raw AS fee_raw, f.asset_id AS asset_id, f.decimals AS decimals, f.status AS status, f.coinbase AS coinbase, f.input_count AS input_count, f.output_count AS output_count, f.raw_evidence_json AS raw_evidence_json, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_transactions AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_address_relationships_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.relationship_id AS relationship_id, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.event_index AS event_index, f.from_subject_id AS from_subject_id, f.from_subject_type AS from_subject_type, f.to_subject_id AS to_subject_id, f.to_subject_type AS to_subject_type, f.from_address AS from_address, f.to_address AS to_address, f.asset_id AS asset_id, f.amount_raw AS amount_raw, f.decimals AS decimals, f.transfer_type AS transfer_type, f.attribution_method AS attribution_method, f.confidence AS confidence, f.prev_tx_hash AS prev_tx_hash, f.prev_output_index AS prev_output_index, f.output_index AS output_index, f.evidence_refs AS evidence_refs, f.created_at_unix_ms AS created_at_unix_ms, f.event_sub_index AS event_sub_index, f.trace_address AS trace_address, f.token_id AS token_id, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_address_relationships AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_transaction_features_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.feature_id AS feature_id, f.tx_hash AS tx_hash, f.subject_id AS subject_id, f.detector_version AS detector_version, f.features_json AS features_json, f.evidence_refs AS evidence_refs, f.created_at_unix_ms AS created_at_unix_ms, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_transaction_features AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_semantic_aml_events_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.event_id AS event_id, f.tx_hash AS tx_hash, f.subject_id AS subject_id, f.event_type AS event_type, f.protocol AS protocol, f.detector_version AS detector_version, f.confidence AS confidence, f.asset_in AS asset_in, f.asset_out AS asset_out, f.amount_in AS amount_in, f.amount_out AS amount_out, f.remote_network_id AS remote_network_id, f.evidence_refs AS evidence_refs, f.evidence_json AS evidence_json, f.created_at_unix_ms AS created_at_unix_ms, f.event_index AS event_index, f.event_sub_index AS event_sub_index, f.detector AS detector, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_semantic_aml_events AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_bitcoin_inputs_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.input_index AS input_index, f.address AS address, f.script_hex AS script_hex, f.script_type AS script_type, f.value_sats AS value_sats, f.prev_tx_hash AS prev_tx_hash, f.prev_output_index AS prev_output_index, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_bitcoin_inputs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_bitcoin_outputs_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.output_index AS output_index, f.address AS address, f.script_hex AS script_hex, f.script_type AS script_type, f.value_sats AS value_sats, f.coinbase AS coinbase, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_bitcoin_outputs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_evm_logs_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.log_index AS log_index, f.address AS address, f.topics AS topics, f.data AS data, f.created_at_unix_ms AS created_at_unix_ms, f.transaction_index AS transaction_index, f.removed AS removed, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_evm_logs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_tron_contracts_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.contract_index AS contract_index, f.contract_type AS contract_type, f.payload_json AS payload_json, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_tron_contracts AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_asset_metadata_current AS SELECT * FROM {{database}}.aml_asset_metadata ORDER BY revision DESC LIMIT 1 BY network_id,asset_id;

CREATE OR REPLACE VIEW {{database}}.aml_metadata_jobs_current AS SELECT * FROM {{database}}.aml_metadata_jobs ORDER BY revision DESC LIMIT 1 BY network_id,job_id;

CREATE OR REPLACE VIEW {{database}}.aml_ingestion_failures_current AS SELECT * FROM {{database}}.aml_ingestion_failures ORDER BY revision DESC LIMIT 1 BY network_id,failure_id;

CREATE OR REPLACE VIEW {{database}}.aml_intelligence_sources_current AS SELECT * FROM {{database}}.aml_intelligence_sources ORDER BY revision DESC LIMIT 1 BY network_id,source_id;

CREATE OR REPLACE VIEW {{database}}.aml_intelligence_claims_current AS SELECT * FROM {{database}}.aml_intelligence_claims ORDER BY revision DESC LIMIT 1 BY network_id,claim_id;

CREATE OR REPLACE VIEW {{database}}.aml_cluster_claims_current AS SELECT * FROM {{database}}.aml_cluster_claims ORDER BY revision DESC LIMIT 1 BY network_id,claim_id;

CREATE OR REPLACE VIEW {{database}}.aml_cluster_runs_current AS SELECT * FROM {{database}}.aml_cluster_runs ORDER BY revision DESC LIMIT 1 BY network_id,run_id;

CREATE OR REPLACE VIEW {{database}}.aml_exposure_seeds_current AS SELECT * FROM {{database}}.aml_exposure_seeds ORDER BY revision DESC LIMIT 1 BY network_id,seed_id;

CREATE OR REPLACE VIEW {{database}}.aml_exposure_runs_current AS SELECT * FROM {{database}}.aml_exposure_runs ORDER BY revision DESC LIMIT 1 BY network_id,run_id;

CREATE OR REPLACE VIEW {{database}}.aml_wallet_risk_assessments_current AS SELECT * FROM {{database}}.aml_wallet_risk_assessments ORDER BY revision DESC LIMIT 1 BY network_id,assessment_id;

CREATE OR REPLACE VIEW {{database}}.aml_ingestion_benchmarks_current AS SELECT * FROM {{database}}.aml_ingestion_benchmarks ORDER BY revision DESC LIMIT 1 BY network_id,benchmark_id;

CREATE OR REPLACE VIEW {{database}}.aml_metadata_discoveries_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.discovery_id AS discovery_id, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.asset_id AS asset_id, f.standard_hint AS standard_hint, f.evidence_id AS evidence_id, f.source_id AS source_id, f.evidence_refs AS evidence_refs, f.raw_evidence_json AS raw_evidence_json, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_metadata_discoveries AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_protocol_registry_current AS SELECT * FROM {{database}}.aml_protocol_registry FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_entities_current AS SELECT * FROM {{database}}.aml_entities FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_entity_addresses_current AS SELECT * FROM {{database}}.aml_entity_addresses FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_cluster_versions_current AS SELECT * FROM {{database}}.aml_cluster_versions FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_address_exposures_current AS SELECT * FROM {{database}}.aml_address_exposures FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_bitcoin_scripts_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.script_hash AS script_hash, f.script_hex AS script_hex, f.script_type AS script_type, f.address AS address, f.evidence_refs AS evidence_refs, f.raw_evidence_json AS raw_evidence_json, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_bitcoin_scripts AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_evm_traces_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.trace_id AS trace_id, f.transaction_index AS transaction_index, f.trace_address AS trace_address, f.trace_type AS trace_type, f.from_address AS from_address, f.to_address AS to_address, f.value_raw AS value_raw, f.success AS success, f.error_message AS error_message, f.source_id AS source_id, f.evidence_refs AS evidence_refs, f.raw_evidence_json AS raw_evidence_json, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_evm_traces AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_raw_transactions_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.source_id AS source_id, f.payload_json AS payload_json, f.receipt_json AS receipt_json, f.evidence_refs AS evidence_refs, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_raw_transactions AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_intelligence_reviews_current AS SELECT * FROM {{database}}.aml_intelligence_reviews FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_cluster_memberships_current AS SELECT * FROM {{database}}.aml_cluster_memberships FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_exposure_paths_current AS SELECT * FROM {{database}}.aml_exposure_paths FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_risk_policies_current AS SELECT * FROM {{database}}.aml_risk_policies FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_risk_policy_rules_current AS SELECT * FROM {{database}}.aml_risk_policy_rules FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_wallet_risk_signals_current AS SELECT * FROM {{database}}.aml_wallet_risk_signals FINAL;

CREATE OR REPLACE VIEW {{database}}.aml_intelligence_claims_active AS SELECT c.network_id AS network_id, c.claim_id AS claim_id, c.address AS address, c.claim_kind AS claim_kind, c.entity_id AS entity_id, c.entity_name AS entity_name, c.entity_type AS entity_type, c.address_role AS address_role, c.confidence AS confidence, c.risk_level AS risk_level, c.is_exposure_seed AS is_exposure_seed, c.seed_category AS seed_category, c.source_id AS source_id, c.source_reference AS source_reference, c.evidence_refs AS evidence_refs, c.review_status AS review_status, c.reviewed_by AS reviewed_by, c.review_note AS review_note, c.valid_from_unix_ms AS valid_from_unix_ms, c.valid_to_unix_ms AS valid_to_unix_ms, c.revision AS revision, c.created_at_unix_ms AS created_at_unix_ms, c.source_record_id AS source_record_id, c.supersedes_claim_id AS supersedes_claim_id, c.submitted_by AS submitted_by, c.case_id AS case_id, c.reviewed_at_unix_ms AS reviewed_at_unix_ms, c.evidence_json AS evidence_json, c.raw_evidence_json AS raw_evidence_json FROM (SELECT network_id, claim_id, data.1 AS address, data.2 AS claim_kind, data.3 AS entity_id, data.4 AS entity_name, data.5 AS entity_type, data.6 AS address_role, data.7 AS confidence, data.8 AS risk_level, data.9 AS is_exposure_seed, data.10 AS seed_category, data.11 AS source_id, data.12 AS source_reference, data.13 AS evidence_refs, data.14 AS review_status, data.15 AS reviewed_by, data.16 AS review_note, data.17 AS valid_from_unix_ms, data.18 AS valid_to_unix_ms, data.19 AS revision, data.20 AS created_at_unix_ms, data.21 AS source_record_id, data.22 AS supersedes_claim_id, data.23 AS submitted_by, data.24 AS case_id, data.25 AS reviewed_at_unix_ms, data.26 AS evidence_json, data.27 AS raw_evidence_json FROM (SELECT network_id, claim_id, argMax(tuple(address, claim_kind, entity_id, entity_name, entity_type, address_role, confidence, risk_level, is_exposure_seed, seed_category, source_id, source_reference, evidence_refs, review_status, reviewed_by, review_note, valid_from_unix_ms, valid_to_unix_ms, revision, created_at_unix_ms, source_record_id, supersedes_claim_id, submitted_by, case_id, reviewed_at_unix_ms, evidence_json, raw_evidence_json), revision) AS data FROM {{database}}.aml_intelligence_claims GROUP BY network_id, claim_id)) AS c INNER JOIN (SELECT network_id, source_id, data.1 AS source_name, data.2 AS source_type, data.3 AS reference_url, data.4 AS trust_tier, data.5 AS license, data.6 AS is_active, data.7 AS created_by, data.8 AS revision, data.9 AS created_at_unix_ms, data.10 AS metadata_json, data.11 AS updated_at_unix_ms, data.12 AS raw_evidence_json FROM (SELECT network_id, source_id, argMax(tuple(source_name, source_type, reference_url, trust_tier, license, is_active, created_by, revision, created_at_unix_ms, metadata_json, updated_at_unix_ms, raw_evidence_json), revision) AS data FROM {{database}}.aml_intelligence_sources GROUP BY network_id, source_id)) AS s ON c.network_id=s.network_id AND c.source_id=s.source_id WHERE c.review_status='approved' AND s.is_active=1 AND c.valid_from_unix_ms<=toUnixTimestamp64Milli(now64(3)) AND (isNull(c.valid_to_unix_ms) OR c.valid_to_unix_ms>toUnixTimestamp64Milli(now64(3)));

CREATE OR REPLACE VIEW {{database}}.aml_protocol_registry_active AS SELECT p.network_id AS network_id, p.registry_id AS registry_id, p.subject_id AS subject_id, p.subject_type AS subject_type, p.protocol AS protocol, p.protocol_type AS protocol_type, p.contract_role AS contract_role, p.remote_network_id AS remote_network_id, p.remote_subject_id AS remote_subject_id, p.decoder AS decoder, p.decoder_config_json AS decoder_config_json, p.source_id AS source_id, p.source_reference AS source_reference, p.confidence AS confidence, p.enabled AS enabled, p.review_status AS review_status, p.reviewed_by AS reviewed_by, p.reviewed_at_unix_ms AS reviewed_at_unix_ms, p.review_note AS review_note, p.evidence_refs AS evidence_refs, p.raw_evidence_json AS raw_evidence_json, p.revision AS revision, p.created_at_unix_ms AS created_at_unix_ms FROM (SELECT network_id, registry_id, data.1 AS subject_id, data.2 AS subject_type, data.3 AS protocol, data.4 AS protocol_type, data.5 AS contract_role, data.6 AS remote_network_id, data.7 AS remote_subject_id, data.8 AS decoder, data.9 AS decoder_config_json, data.10 AS source_id, data.11 AS source_reference, data.12 AS confidence, data.13 AS enabled, data.14 AS review_status, data.15 AS reviewed_by, data.16 AS reviewed_at_unix_ms, data.17 AS review_note, data.18 AS evidence_refs, data.19 AS raw_evidence_json, data.20 AS revision, data.21 AS created_at_unix_ms FROM (SELECT network_id, registry_id, argMax(tuple(subject_id, subject_type, protocol, protocol_type, contract_role, remote_network_id, remote_subject_id, decoder, decoder_config_json, source_id, source_reference, confidence, enabled, review_status, reviewed_by, reviewed_at_unix_ms, review_note, evidence_refs, raw_evidence_json, revision, created_at_unix_ms), revision) AS data FROM {{database}}.aml_protocol_registry GROUP BY network_id, registry_id)) AS p INNER JOIN (SELECT network_id, source_id, data.1 AS source_name, data.2 AS source_type, data.3 AS reference_url, data.4 AS trust_tier, data.5 AS license, data.6 AS is_active, data.7 AS created_by, data.8 AS revision, data.9 AS created_at_unix_ms, data.10 AS metadata_json, data.11 AS updated_at_unix_ms, data.12 AS raw_evidence_json FROM (SELECT network_id, source_id, argMax(tuple(source_name, source_type, reference_url, trust_tier, license, is_active, created_by, revision, created_at_unix_ms, metadata_json, updated_at_unix_ms, raw_evidence_json), revision) AS data FROM {{database}}.aml_intelligence_sources GROUP BY network_id, source_id)) AS s ON p.network_id=s.network_id AND p.source_id=s.source_id WHERE p.review_status='approved' AND p.enabled=1 AND s.is_active=1;

CREATE OR REPLACE VIEW {{database}}.aml_entity_addresses_active AS SELECT a.network_id AS network_id, a.entity_id AS entity_id, a.address AS address, a.address_role AS address_role, a.claim_id AS claim_id, a.source_id AS source_id, a.confidence AS confidence, a.is_active AS is_active, a.first_seen_block AS first_seen_block, a.last_seen_block AS last_seen_block, a.review_status AS review_status, a.reviewed_by AS reviewed_by, a.reviewed_at_unix_ms AS reviewed_at_unix_ms, a.review_note AS review_note, a.evidence_refs AS evidence_refs, a.raw_evidence_json AS raw_evidence_json, a.revision AS revision, a.created_at_unix_ms AS created_at_unix_ms FROM (SELECT network_id, entity_id, address, claim_id, data.1 AS address_role, data.2 AS source_id, data.3 AS confidence, data.4 AS is_active, data.5 AS first_seen_block, data.6 AS last_seen_block, data.7 AS review_status, data.8 AS reviewed_by, data.9 AS reviewed_at_unix_ms, data.10 AS review_note, data.11 AS evidence_refs, data.12 AS raw_evidence_json, data.13 AS revision, data.14 AS created_at_unix_ms FROM (SELECT network_id, entity_id, address, claim_id, argMax(tuple(address_role, source_id, confidence, is_active, first_seen_block, last_seen_block, review_status, reviewed_by, reviewed_at_unix_ms, review_note, evidence_refs, raw_evidence_json, revision, created_at_unix_ms), revision) AS data FROM {{database}}.aml_entity_addresses GROUP BY network_id, entity_id, address, claim_id)) AS a INNER JOIN (SELECT network_id, entity_id, data.1 AS entity_name, data.2 AS entity_type, data.3 AS source_id, data.4 AS source_reference, data.5 AS is_active, data.6 AS review_status, data.7 AS reviewed_by, data.8 AS reviewed_at_unix_ms, data.9 AS review_note, data.10 AS evidence_refs, data.11 AS raw_evidence_json, data.12 AS revision, data.13 AS created_at_unix_ms FROM (SELECT network_id, entity_id, argMax(tuple(entity_name, entity_type, source_id, source_reference, is_active, review_status, reviewed_by, reviewed_at_unix_ms, review_note, evidence_refs, raw_evidence_json, revision, created_at_unix_ms), revision) AS data FROM {{database}}.aml_entities GROUP BY network_id, entity_id)) AS e ON a.network_id=e.network_id AND a.entity_id=e.entity_id INNER JOIN {{database}}.aml_intelligence_claims_active AS c ON a.network_id=c.network_id AND a.claim_id=c.claim_id AND a.address=c.address AND a.entity_id=c.entity_id AND a.source_id=c.source_id WHERE a.is_active=1 AND a.review_status='approved' AND e.is_active=1 AND e.review_status='approved';

CREATE OR REPLACE VIEW {{database}}.aml_exchange_addresses_active AS SELECT a.network_id AS network_id, a.entity_id AS entity_id, a.address AS address, a.address_role AS address_role, a.claim_id AS claim_id, a.source_id AS source_id, a.confidence AS confidence, a.is_active AS is_active, a.first_seen_block AS first_seen_block, a.last_seen_block AS last_seen_block, a.review_status AS review_status, a.reviewed_by AS reviewed_by, a.reviewed_at_unix_ms AS reviewed_at_unix_ms, a.review_note AS review_note, a.evidence_refs AS evidence_refs, a.raw_evidence_json AS raw_evidence_json, a.revision AS revision, a.created_at_unix_ms AS created_at_unix_ms FROM (SELECT network_id, entity_id, address, claim_id, data.1 AS address_role, data.2 AS source_id, data.3 AS confidence, data.4 AS is_active, data.5 AS first_seen_block, data.6 AS last_seen_block, data.7 AS review_status, data.8 AS reviewed_by, data.9 AS reviewed_at_unix_ms, data.10 AS review_note, data.11 AS evidence_refs, data.12 AS raw_evidence_json, data.13 AS revision, data.14 AS created_at_unix_ms FROM (SELECT network_id, entity_id, address, claim_id, argMax(tuple(address_role, source_id, confidence, is_active, first_seen_block, last_seen_block, review_status, reviewed_by, reviewed_at_unix_ms, review_note, evidence_refs, raw_evidence_json, revision, created_at_unix_ms), revision) AS data FROM {{database}}.aml_entity_addresses GROUP BY network_id, entity_id, address, claim_id)) AS a INNER JOIN (SELECT network_id, entity_id, data.1 AS entity_name, data.2 AS entity_type, data.3 AS source_id, data.4 AS source_reference, data.5 AS is_active, data.6 AS review_status, data.7 AS reviewed_by, data.8 AS reviewed_at_unix_ms, data.9 AS review_note, data.10 AS evidence_refs, data.11 AS raw_evidence_json, data.12 AS revision, data.13 AS created_at_unix_ms FROM (SELECT network_id, entity_id, argMax(tuple(entity_name, entity_type, source_id, source_reference, is_active, review_status, reviewed_by, reviewed_at_unix_ms, review_note, evidence_refs, raw_evidence_json, revision, created_at_unix_ms), revision) AS data FROM {{database}}.aml_entities GROUP BY network_id, entity_id)) AS e ON a.network_id=e.network_id AND a.entity_id=e.entity_id INNER JOIN {{database}}.aml_intelligence_claims_active AS c ON a.network_id=c.network_id AND a.claim_id=c.claim_id AND a.address=c.address AND a.entity_id=c.entity_id AND a.source_id=c.source_id WHERE a.is_active=1 AND a.review_status='approved' AND e.is_active=1 AND e.review_status='approved' AND e.entity_type='exchange';

