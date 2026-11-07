-- Generated core-evidence upgrade; do not edit.
-- Migration: 20261026_0003_core_evidence
-- Checksum: 9548e226c0d1d4259fe488bab1d9a137232e6e68d311054190b0034db7e0800e

ALTER TABLE {{database}}.aml_sync_state ADD COLUMN IF NOT EXISTS `source_cursor_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_ingested_blocks ADD COLUMN IF NOT EXISTS `expected_counts_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_ingested_blocks ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_address_relationships ADD COLUMN IF NOT EXISTS `event_sub_index` UInt32 DEFAULT 0;

ALTER TABLE {{database}}.aml_address_relationships ADD COLUMN IF NOT EXISTS `trace_address` Array(UInt32) DEFAULT [];

ALTER TABLE {{database}}.aml_address_relationships ADD COLUMN IF NOT EXISTS `token_id` Nullable(UInt256) DEFAULT NULL;

ALTER TABLE {{database}}.aml_address_relationships ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_transaction_features ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_semantic_aml_events ADD COLUMN IF NOT EXISTS `event_index` UInt32 DEFAULT 0;

ALTER TABLE {{database}}.aml_semantic_aml_events ADD COLUMN IF NOT EXISTS `event_sub_index` UInt32 DEFAULT 0;

ALTER TABLE {{database}}.aml_semantic_aml_events ADD COLUMN IF NOT EXISTS `detector` String DEFAULT '';

ALTER TABLE {{database}}.aml_semantic_aml_events ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_ingestion_failures ADD COLUMN IF NOT EXISTS `tx_hash` Nullable(String) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_failures ADD COLUMN IF NOT EXISTS `retryable` Nullable(UInt8) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_failures ADD COLUMN IF NOT EXISTS `first_failed_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_failures ADD COLUMN IF NOT EXISTS `last_failed_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_failures ADD COLUMN IF NOT EXISTS `resolved_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_failures ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `source_kind` LowCardinality(String) DEFAULT 'unknown';

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `status` LowCardinality(String) DEFAULT 'unknown';

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `started_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `completed_at_unix_ms` Nullable(UInt64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `metrics_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `error_message` String DEFAULT '';

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `raw_evidence_json` String DEFAULT '{}';

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `blocks_per_second` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `transactions_per_second` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD COLUMN IF NOT EXISTS `rows_per_second` Nullable(Float64) DEFAULT NULL;

ALTER TABLE {{database}}.aml_sync_state ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (isValidJSON(`source_cursor_json`));

ALTER TABLE {{database}}.aml_ingested_blocks ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (isValidJSON(`expected_counts_json`)) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_address_relationships ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (isNull(`token_id`) OR (match(asset_id,'^(erc721|erc1155|trc721):[^/]+/[0-9]+$') AND toString(`token_id`)=extract(asset_id,'/([0-9]+)$'))) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_transaction_features ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_semantic_aml_events ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_ingestion_failures ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (isNull(`tx_hash`) OR ((network_id='tron:mainnet' AND match(`tx_hash`,'^[0-9a-f]{64}$')) OR (network_id='eip155:1' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='eip155:56' AND match(`tx_hash`,'^0x[0-9a-f]{64}$')) OR (network_id='bip122:000000000019d6689c085ae165831e93' AND match(`tx_hash`,'^[0-9a-f]{64}$')))) AND (isNull(`retryable`) OR (`retryable` IN (0,1))) AND (isValidJSON(`raw_evidence_json`));

ALTER TABLE {{database}}.aml_ingestion_benchmarks ADD CONSTRAINT IF NOT EXISTS `aml_core_v3` CHECK (`status` IN ('unknown','pending','running','complete','failed','cancelled')) AND (isValidJSON(`metrics_json`)) AND (isValidJSON(`raw_evidence_json`)) AND (isNull(`blocks_per_second`) OR (isFinite(`blocks_per_second`) AND `blocks_per_second`>=0)) AND (isNull(`transactions_per_second`) OR (isFinite(`transactions_per_second`) AND `transactions_per_second`>=0)) AND (isNull(`rows_per_second`) OR (isFinite(`rows_per_second`) AND `rows_per_second`>=0));

CREATE OR REPLACE VIEW {{database}}.aml_sync_state_current AS SELECT * FROM {{database}}.aml_sync_state ORDER BY revision DESC LIMIT 1 BY network_id;

CREATE OR REPLACE VIEW {{database}}.aml_ingested_blocks_canonical AS SELECT network_id, block_number, data.1 AS block_hash, data.2 AS block_timestamp_unix_ms, data.3 AS commit_id, data.4 AS revision, data.5 AS parent_hash, data.6 AS canonical, data.7 AS ingestion_status, data.8 AS transaction_count, data.9 AS receipt_coverage, data.10 AS trace_coverage, data.11 AS prevout_coverage, data.12 AS source_id, data.13 AS created_at_unix_ms, data.14 AS expected_counts_json, data.15 AS raw_evidence_json FROM (SELECT b.network_id AS network_id, b.block_number AS block_number, argMax(tuple(b.block_hash, b.block_timestamp_unix_ms, b.commit_id, b.revision, b.parent_hash, b.canonical, b.ingestion_status, b.transaction_count, b.receipt_coverage, b.trace_coverage, b.prevout_coverage, b.source_id, b.created_at_unix_ms, b.expected_counts_json, b.raw_evidence_json), b.revision) AS data FROM {{database}}.aml_ingested_blocks AS b INNER JOIN {{database}}.aml_sync_state_current AS s ON b.network_id=s.network_id WHERE s.initialized=1 AND b.revision<=s.revision AND b.block_number<=s.last_block_number GROUP BY b.network_id,b.block_number) WHERE data.6=1 AND data.7='complete';

CREATE OR REPLACE VIEW {{database}}.aml_transactions_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.from_address AS from_address, f.to_address AS to_address, f.amount_raw AS amount_raw, f.fee_raw AS fee_raw, f.asset_id AS asset_id, f.decimals AS decimals, f.status AS status, f.coinbase AS coinbase, f.input_count AS input_count, f.output_count AS output_count, f.raw_evidence_json AS raw_evidence_json, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_transactions AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_address_relationships_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.relationship_id AS relationship_id, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.event_index AS event_index, f.from_subject_id AS from_subject_id, f.from_subject_type AS from_subject_type, f.to_subject_id AS to_subject_id, f.to_subject_type AS to_subject_type, f.from_address AS from_address, f.to_address AS to_address, f.asset_id AS asset_id, f.amount_raw AS amount_raw, f.decimals AS decimals, f.transfer_type AS transfer_type, f.attribution_method AS attribution_method, f.confidence AS confidence, f.prev_tx_hash AS prev_tx_hash, f.prev_output_index AS prev_output_index, f.output_index AS output_index, f.evidence_refs AS evidence_refs, f.created_at_unix_ms AS created_at_unix_ms, f.event_sub_index AS event_sub_index, f.trace_address AS trace_address, f.token_id AS token_id, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_address_relationships AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_transaction_features_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.feature_id AS feature_id, f.tx_hash AS tx_hash, f.subject_id AS subject_id, f.detector_version AS detector_version, f.features_json AS features_json, f.evidence_refs AS evidence_refs, f.created_at_unix_ms AS created_at_unix_ms, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_transaction_features AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_semantic_aml_events_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.event_id AS event_id, f.tx_hash AS tx_hash, f.subject_id AS subject_id, f.event_type AS event_type, f.protocol AS protocol, f.detector_version AS detector_version, f.confidence AS confidence, f.asset_in AS asset_in, f.asset_out AS asset_out, f.amount_in AS amount_in, f.amount_out AS amount_out, f.remote_network_id AS remote_network_id, f.evidence_refs AS evidence_refs, f.evidence_json AS evidence_json, f.created_at_unix_ms AS created_at_unix_ms, f.event_index AS event_index, f.event_sub_index AS event_sub_index, f.detector AS detector, f.raw_evidence_json AS raw_evidence_json FROM {{database}}.aml_semantic_aml_events AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_bitcoin_inputs_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.input_index AS input_index, f.address AS address, f.script_hex AS script_hex, f.script_type AS script_type, f.value_sats AS value_sats, f.prev_tx_hash AS prev_tx_hash, f.prev_output_index AS prev_output_index, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_bitcoin_inputs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_bitcoin_outputs_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.transaction_index AS transaction_index, f.output_index AS output_index, f.address AS address, f.script_hex AS script_hex, f.script_type AS script_type, f.value_sats AS value_sats, f.coinbase AS coinbase, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_bitcoin_outputs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

CREATE OR REPLACE VIEW {{database}}.aml_evm_logs_canonical AS SELECT f.network_id AS network_id, f.block_number AS block_number, f.block_hash AS block_hash, f.block_timestamp_unix_ms AS block_timestamp_unix_ms, f.commit_id AS commit_id, f.revision AS revision, f.tx_hash AS tx_hash, f.log_index AS log_index, f.address AS address, f.topics AS topics, f.data AS data, f.created_at_unix_ms AS created_at_unix_ms FROM {{database}}.aml_evm_logs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision;

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

