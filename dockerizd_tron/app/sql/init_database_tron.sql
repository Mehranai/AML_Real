-- TRON AML production baseline.
CREATE DATABASE IF NOT EXISTS tron_db;

CREATE TABLE IF NOT EXISTS tron_db.address_cluster_claims
(
    `claim_id` String,
    `chain` LowCardinality(String),
    `address` String,
    `cluster_id` String,
    `cluster_type` LowCardinality(String),
    `address_role` LowCardinality(String),
    `claim_method` LowCardinality(String),
    `confidence` Float32,
    `source` String,
    `source_record_id` String DEFAULT '',
    `evidence_tx_hashes` Array(String),
    `evidence_addresses` Array(String),
    `evidence_json` String,
    `supersedes_claim_id` String DEFAULT '',
    `review_status` LowCardinality(String) DEFAULT 'PENDING',
    `created_by` String,
    `created_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_cluster_claim_id claim_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_cluster_claim_cluster cluster_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, address, cluster_id, claim_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.address_cluster_memberships
(
    `chain` LowCardinality(String),
    `address` String,
    `cluster_id` String,
    `cluster_type` LowCardinality(String),
    `address_role` LowCardinality(String),
    `confidence` Float32,
    `source_claim_id` String,
    `review_id` String,
    `cluster_version` UInt32,
    `is_active` UInt8,
    `created_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_membership_cluster cluster_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, address, cluster_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.address_entity
(
    `address` String,
    `entity_id` String,
    `entity_name` String,
    `entity_type` String,
    `confidence` Float32,
    `source` String,
    `is_active` UInt8 DEFAULT 1,
    `created_at` DateTime DEFAULT now(),
    INDEX idx_entity_type entity_type TYPE set(100) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(created_at)
ORDER BY address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.address_exposure
(
    `source_address` String,
    `exposed_address` String,
    `hop_distance` UInt8,
    `exposure_score` Float64,
    `path_count` UInt32,
    `last_tx_hash` String,
    `last_seen_block` UInt64,
    `exposure_type` String,
    `best_path_amount_share` Float64 DEFAULT 0,
    `best_path_time_weight` Float64 DEFAULT 0,
    `service_mediated` UInt8 DEFAULT 0,
    `propagation_run_id` String DEFAULT '',
    `updated_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (source_address, exposed_address)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.address_relationships
(
    `relationship_id` String,
    `from_address` String,
    `to_address` String,
    `token_address` String,
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `amount` UInt256,
    `transfer_type` LowCardinality(String),
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_transfer_type transfer_type TYPE set(100) GRANULARITY 4,
    INDEX idx_to_address to_address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_tx_hash tx_hash TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_from_address from_address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(timestamp, 1000)))
ORDER BY (from_address, timestamp, tx_hash, relationship_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.analysis_subjects
(
    `chain` String,
    `subject_type` String,
    `subject_id` String,
    `address` String,
    `entity_id` String,
    `latest_snapshot_id` String,
    `latest_status` String,
    `latest_risk_available` UInt8 DEFAULT 0,
    `latest_risk_level` String,
    `latest_risk_probability` Float32,
    `latest_confidence` Float32,
    `latest_data_cutoff_block` UInt64,
    `latest_data_cutoff_unix_ms` UInt64,
    `latest_input_version` String DEFAULT '',
    `created_at_unix_ms` UInt64,
    `updated_at_unix_ms` UInt64
)
ENGINE = ReplacingMergeTree(updated_at_unix_ms)
ORDER BY (chain, subject_type, subject_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.cluster_versions
(
    `chain` LowCardinality(String),
    `cluster_id` String,
    `version` UInt32,
    `cluster_type` LowCardinality(String),
    `display_name` String,
    `change_type` LowCardinality(String),
    `change_reason` String DEFAULT '',
    `source_claim_ids` Array(String),
    `active_member_count` UInt64,
    `created_by` String,
    `created_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, cluster_id, version)
SETTINGS index_granularity = 8192
;


CREATE TABLE IF NOT EXISTS tron_db.entity_labels
(
    `label_id` String,
    `chain` LowCardinality(String),
    `address` String,
    `entity_id` String,
    `entity_name` String,
    `entity_type` LowCardinality(String),
    `address_role` LowCardinality(String) DEFAULT 'UNKNOWN',
    `confidence` Float32,
    `risk_percent` UInt8 DEFAULT 0,
    `source` String,
    `source_record_id` String DEFAULT '',
    `supersedes_label_id` String DEFAULT '',
    `submitted_by` String DEFAULT '',
    `case_id` String DEFAULT '',
    `evidence_refs` Array(String),
    `review_status` LowCardinality(String),
    `created_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, address, label_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.exchange_addresses
(
    `address` String,
    `entity_id` String,
    `exchange_name` String,
    `address_role` String,
    `confidence` Float32,
    `detection_source` String,
    `first_seen_block` UInt64,
    `last_seen_block` UInt64,
    `is_active` UInt8 DEFAULT 1,
    `created_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree(created_at)
ORDER BY address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.exposure_runs
(
    `source_address` String,
    `propagation_run_id` String,
    `status` String,
    `max_hops` UInt8,
    `row_count` UInt64,
    `completed_at_unix_ms` UInt64,
    `updated_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY source_address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.exposure_seeds
(
    `address` String,
    `entity_name` String,
    `entity_type` String,
    `risk_level` UInt8,
    `source` String,
    `source_label_id` String DEFAULT '',
    `is_active` UInt8 DEFAULT 1,
    `created_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree(created_at)
ORDER BY address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.ingested_blocks
(
    `chain` LowCardinality(String) DEFAULT 'tron',
    `block_number` UInt64,
    `block_hash` String,
    `parent_hash` String,
    `block_timestamp` UInt64,
    `transaction_count` UInt32,
    `finality_status` LowCardinality(String),
    `ingestion_status` LowCardinality(String),
    `error_message` String DEFAULT '',
    `indexed_at_unix_ms` UInt64,
    `updated_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_ingested_block_status ingestion_status TYPE set(8) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (chain, block_number)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.ingestion_benchmarks
(
    `run_id` String,
    `chain` LowCardinality(String),
    `source_kind` LowCardinality(String),
    `start_block` UInt64,
    `end_block` UInt64,
    `requested_blocks` UInt32,
    `completed_blocks` UInt32,
    `transaction_count` UInt64,
    `elapsed_ms` UInt64,
    `blocks_per_second` Float64,
    `transactions_per_second` Float64,
    `rows_before` UInt64,
    `rows_after` UInt64,
    `compressed_bytes_before` UInt64,
    `compressed_bytes_after` UInt64,
    `investigation_address` String,
    `investigation_latency_ms` UInt64,
    `status` LowCardinality(String),
    `error_message` String CODEC(ZSTD(3)),
    `metrics_json` String CODEC(ZSTD(3)),
    `started_at_unix_ms` UInt64,
    `completed_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, run_id)
TTL toDateTime(inserted_at) + toIntervalDay(365)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.ingestion_failures
(
    `failure_id` String,
    `chain` LowCardinality(String),
    `block_number` UInt64,
    `block_hash` String,
    `tx_hash` String,
    `stage` LowCardinality(String),
    `error_class` LowCardinality(String),
    `error_message` String CODEC(ZSTD(3)),
    `retryable` UInt8,
    `attempt_count` UInt32,
    `status` LowCardinality(String),
    `first_failed_at_unix_ms` UInt64,
    `last_failed_at_unix_ms` UInt64,
    `resolved_at_unix_ms` UInt64,
    `updated_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_ingestion_failure_status status TYPE set(16) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (chain, failure_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.intelligence_reviews
(
    `review_id` String,
    `chain` LowCardinality(String),
    `subject_type` LowCardinality(String),
    `subject_id` String,
    `decision` LowCardinality(String),
    `reviewer` String,
    `reason` String DEFAULT '',
    `evidence_refs` Array(String),
    `created_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_review_subject (subject_type, subject_id) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, subject_type, subject_id, review_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.intelligence_sources
(
    `chain` LowCardinality(String),
    `source_id` String,
    `source_name` String,
    `source_type` LowCardinality(String),
    `trust_tier` LowCardinality(String),
    `reference_url` String DEFAULT '',
    `license` String DEFAULT '',
    `is_active` UInt8 DEFAULT 1,
    `created_by` String,
    `created_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (chain, source_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.semantic_aml_events
(
    `event_id` String,
    `chain` LowCardinality(String) DEFAULT 'tron',
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `event_type` LowCardinality(String),
    `subject_address` String,
    `protocol` String,
    `asset_in` String DEFAULT '',
    `asset_out` String DEFAULT '',
    `detector` String,
    `detector_version` String,
    `confidence` Float32,
    `evidence_json` String,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_semantic_subject subject_address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(timestamp, 1000)))
ORDER BY event_id
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.sync_state
(
    `chain` String,
    `last_synced_block` UInt64,
    `updated_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY chain
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.token_metadata
(
    `token_address` String,
    `name` String,
    `symbol` String,
    `decimals` UInt8,
    `is_verified` UInt8 DEFAULT 0,
    `created_at` DateTime64(3) DEFAULT now64(3),
    `updated_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY token_address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.token_metadata_discoveries
(
    `token_address` String,
    `discovered_block` UInt64,
    `discovered_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY token_address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.token_metadata_jobs
(
    `token_address` String,
    `status` LowCardinality(String),
    `attempt_count` UInt8,
    `last_error` String DEFAULT '',
    `updated_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY token_address
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.transaction_features
(
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `transaction_type` LowCardinality(String),
    `transaction_subtype` LowCardinality(String),
    `classification_confidence` Float32,
    `classification_source` LowCardinality(String),
    `protocol` String,
    `method_id` String,
    `is_swap` UInt8,
    `is_bridge` UInt8,
    `is_mint` UInt8,
    `is_burn` UInt8,
    `is_liquidity_add` UInt8,
    `is_liquidity_remove` UInt8,
    `is_contract_call` UInt8,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_transaction_type transaction_type TYPE set(100) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(timestamp, 1000)))
ORDER BY (block_number, tx_hash)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.transactions
(
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `initiator_address` String,
    `target_address` String,
    `contract_address` String,
    `contract_type` LowCardinality(String),
    `fee` UInt256,
    `energy_usage_total` UInt64,
    `net_usage` UInt64,
    `status` UInt8,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_initiator_address initiator_address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_target_address target_address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_contract_address contract_address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(timestamp, 1000)))
ORDER BY (block_number, tx_hash)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.wallet_analysis_evidence
(
    `evidence_id` String,
    `snapshot_id` String,
    `chain` String,
    `address` String,
    `evidence_type` String,
    `evidence_key` String,
    `evidence_value` String,
    `severity` String,
    `related_tx_hash` String,
    `related_address` String,
    `created_at_unix_ms` UInt64
)
ENGINE = MergeTree
ORDER BY (chain, address, snapshot_id, evidence_type, evidence_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.wallet_analysis_snapshots
(
    `snapshot_id` String,
    `chain` String,
    `address` String,
    `entity_id` String,
    `analysis_version` String,
    `analysis_status` String,
    `risk_available` UInt8 DEFAULT 0,
    `risk_level` String,
    `risk_probability` Float32,
    `risk_percent` UInt8,
    `confidence` Float32,
    `wallet_type` String,
    `fingerprint_label` String,
    `graph_depth` UInt8,
    `graph_node_count` UInt32,
    `graph_edge_count` UInt32,
    `exchange_interaction_count` UInt32,
    `holdings_asset_count` UInt64,
    `holdings_metadata_gap_count` UInt32,
    `observed_transfers` UInt64,
    `incoming_transfers` UInt64,
    `outgoing_transfers` UInt64,
    `exposure_score` Float32,
    `exposure_source_count` UInt32,
    `exposure_path_count` UInt64,
    `exposure_min_hop_distance` UInt8,
    `data_cutoff_block` UInt64,
    `data_cutoff_unix_ms` UInt64,
    `analysis_input_version` String DEFAULT '',
    `source_tables` Array(String),
    `model_id` String,
    `model_version` String,
    `feature_schema_version` String,
    `snapshot_json` String,
    `warnings` Array(String),
    `evidence_refs` Array(String),
    `created_at_unix_ms` UInt64
)
ENGINE = MergeTree
ORDER BY (chain, address, created_at_unix_ms, snapshot_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.wallet_asset_balance_deltas_v3
(
    `delta_id` String,
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `address` String,
    `asset_type` LowCardinality(String),
    `asset_id` String,
    `amount_raw` UInt256,
    `direction` Int8,
    `inserted_at` DateTime64(3) DEFAULT now64(3),
    INDEX idx_balance_address address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(timestamp, 1000)))
ORDER BY delta_id
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.wallet_ml_feature_snapshots
(
    `snapshot_id` String,
    `address` String,
    `window_days` UInt16,
    `feature_schema_version` String,
    `feature_names` Array(String),
    `features_json` String CODEC(ZSTD(1)),
    `evidence_refs` Array(String),
    `generated_at_unix_ms` UInt64,
    `inserted_at` DateTime DEFAULT now(),
    INDEX idx_wallet_ml_feature_schema feature_schema_version TYPE set(20) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(generated_at_unix_ms, 1000)))
ORDER BY (address, feature_schema_version, window_days, generated_at_unix_ms, snapshot_id)
SETTINGS index_granularity = 8192
;


CREATE TABLE IF NOT EXISTS tron_db.wallet_ml_model_deployments
(
    `environment` LowCardinality(String),
    `feature_schema_version` String,
    `model_id` String,
    `model_version` String,
    `status` LowCardinality(String),
    `deployed_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (environment, feature_schema_version)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.wallet_ml_model_registry
(
    `model_id` String,
    `model_version` String,
    `model_family` LowCardinality(String),
    `feature_schema_version` String,
    `calibration_version` String,
    `artifact_json` String CODEC(ZSTD(1)),
    `artifact_sha256` String DEFAULT '',
    `metrics_json` String CODEC(ZSTD(1)),
    `model_quality_score` Float32,
    `trained_at_unix_ms` UInt64,
    `inserted_at` DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (feature_schema_version, model_version, model_id)
SETTINGS index_granularity = 8192
;

CREATE TABLE IF NOT EXISTS tron_db.wallet_ml_predictions
(
    `prediction_id` String,
    `snapshot_id` String,
    `model_id` String,
    `model_version` String,
    `model_family` String,
    `calibration_version` String,
    `address` String,
    `window_days` UInt16,
    `risk_probability` Float32,
    `risk_percent` UInt8,
    `risk_level` String,
    `confidence` Float32,
    `feature_importance_json` String CODEC(ZSTD(1)),
    `model_patterns_json` String CODEC(ZSTD(1)),
    `evidence_refs` Array(String),
    `predicted_at_unix_ms` UInt64,
    `inserted_at` DateTime DEFAULT now(),
    INDEX idx_wallet_ml_prediction_risk risk_percent TYPE minmax GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(predicted_at_unix_ms, 1000)))
ORDER BY (address, model_version, window_days, predicted_at_unix_ms, prediction_id)
SETTINGS index_granularity = 8192
;


CREATE MATERIALIZED VIEW IF NOT EXISTS tron_db.mv_wallet_asset_delta_transfer_from_v3 TO tron_db.wallet_asset_balance_deltas_v3
(
    `delta_id` String,
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `address` String,
    `asset_type` String,
    `asset_id` String,
    `amount_raw` UInt256,
    `direction` Int8,
    `inserted_at` DateTime64(3)
) AS
SELECT
    concat(relationship_id, ':from') AS delta_id,
    tx_hash,
    block_number,
    timestamp,
    from_address AS address,
    multiIf(token_address = 'TRX', 'native', startsWith(token_address, 'TRC10:'), 'trc10', 'trc20') AS asset_type,
    token_address AS asset_id,
    amount AS amount_raw,
    toInt8(-1) AS direction,
    now64(3) AS inserted_at
FROM tron_db.address_relationships
WHERE (amount > 0) AND (from_address != '') AND (from_address != 'T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb')
;

CREATE MATERIALIZED VIEW IF NOT EXISTS tron_db.mv_wallet_asset_delta_transfer_to_v3 TO tron_db.wallet_asset_balance_deltas_v3
(
    `delta_id` String,
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `address` String,
    `asset_type` String,
    `asset_id` String,
    `amount_raw` UInt256,
    `direction` Int8,
    `inserted_at` DateTime64(3)
) AS
SELECT
    concat(relationship_id, ':to') AS delta_id,
    tx_hash,
    block_number,
    timestamp,
    to_address AS address,
    multiIf(token_address = 'TRX', 'native', startsWith(token_address, 'TRC10:'), 'trc10', 'trc20') AS asset_type,
    token_address AS asset_id,
    amount AS amount_raw,
    toInt8(1) AS direction,
    now64(3) AS inserted_at
FROM tron_db.address_relationships
WHERE (amount > 0) AND (to_address != '') AND (to_address != 'T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb')
;

CREATE VIEW IF NOT EXISTS tron_db.address_relationships_canonical
(
    `relationship_id` String,
    `from_address` String,
    `to_address` String,
    `token_address` String,
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `amount` UInt256,
    `transfer_type` String,
    `operation_type` String,
    `protocol` String,
    `transaction_type` String,
    `transaction_subtype` String,
    `classification_confidence` Float32,
    `classification_source` String,
    `method_id` String,
    `is_contract_call` UInt8,
    `inserted_at` DateTime64(3)
) AS
SELECT
    transfer.relationship_id AS relationship_id,
    transfer.from_address AS from_address,
    transfer.to_address AS to_address,
    transfer.token_address AS token_address,
    transfer.tx_hash AS tx_hash,
    transfer.block_number AS block_number,
    transfer.timestamp AS timestamp,
    transfer.amount AS amount,
    transfer.transfer_type AS transfer_type,
    multiIf(ifNull(feature.is_swap, toUInt8(0)) > 0, 'swap', ifNull(feature.is_bridge, toUInt8(0)) > 0, 'bridge', ifNull(feature.is_liquidity_add, toUInt8(0)) > 0, 'liquidity_add', ifNull(feature.is_liquidity_remove, toUInt8(0)) > 0, 'liquidity_remove', ifNull(feature.is_mint, toUInt8(0)) > 0, 'mint', ifNull(feature.is_burn, toUInt8(0)) > 0, 'burn', ifNull(feature.transaction_type, '') NOT IN ('', 'unknown', 'exchange_flow'), feature.transaction_type, transfer.transfer_type) AS operation_type,
    ifNull(feature.protocol, '') AS protocol,
    ifNull(feature.transaction_type, '') AS transaction_type,
    ifNull(feature.transaction_subtype, '') AS transaction_subtype,
    ifNull(feature.classification_confidence, toFloat32(0)) AS classification_confidence,
    ifNull(feature.classification_source, '') AS classification_source,
    ifNull(feature.method_id, '') AS method_id,
    ifNull(feature.is_contract_call, toUInt8(0)) AS is_contract_call,
    transfer.inserted_at AS inserted_at
FROM tron_db.address_relationships AS transfer FINAL
INNER JOIN
(
    SELECT block_number
    FROM tron_db.ingested_blocks FINAL
    WHERE chain = 'tron' AND ingestion_status = 'COMPLETE'
) AS committed ON committed.block_number = transfer.block_number
LEFT JOIN
(
    SELECT
        tx_hash,
        transaction_type,
        transaction_subtype,
        classification_confidence,
        classification_source,
        protocol,
        method_id,
        is_swap,
        is_bridge,
        is_mint,
        is_burn,
        is_liquidity_add,
        is_liquidity_remove,
        is_contract_call
    FROM tron_db.transaction_features FINAL
) AS feature ON feature.tx_hash = transfer.tx_hash
WHERE (transfer.amount > 0)
  AND (transfer.from_address != '')
  AND (transfer.to_address != '')
  AND (transfer.from_address != 'T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb')
  AND (transfer.to_address != 'T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb')
;

CREATE VIEW IF NOT EXISTS tron_db.exchange_flows_canonical
(
    `flow_id` String,
    `tx_hash` String,
    `block_number` UInt64,
    `from_address` String,
    `to_address` String,
    `exchange_name` String,
    `flow_type` String,
    `token_address` String,
    `amount` UInt256,
    `confidence` Float32
) AS
SELECT
    lower(hex(SHA256(concat(relationship.relationship_id, '|', ifNull(from_exchange.entity_id, ''), '|', ifNull(to_exchange.entity_id, ''))))) AS flow_id,
    relationship.tx_hash AS tx_hash,
    relationship.block_number AS block_number,
    relationship.from_address AS from_address,
    relationship.to_address AS to_address,
    multiIf((from_exchange.address != '') AND (to_exchange.address != '') AND (from_exchange.entity_id != to_exchange.entity_id), concat(from_exchange.exchange_name, ' -> ', to_exchange.exchange_name), to_exchange.address != '', to_exchange.exchange_name, from_exchange.exchange_name) AS exchange_name,
    multiIf((from_exchange.address != '') AND (to_exchange.address != '') AND (from_exchange.entity_id = to_exchange.entity_id) AND (upper(from_exchange.address_role) = 'DEPOSIT') AND (upper(to_exchange.address_role) IN ('HOT', 'COLD')), 'sweep', (from_exchange.address != '') AND (to_exchange.address != ''), 'exchange_transfer', to_exchange.address != '', 'deposit', 'withdrawal') AS flow_type,
    relationship.token_address AS token_address,
    relationship.amount AS amount,
    multiIf((from_exchange.address != '') AND (to_exchange.address != ''), least(from_exchange.confidence, to_exchange.confidence), to_exchange.address != '', to_exchange.confidence, from_exchange.confidence) AS confidence
FROM tron_db.address_relationships_canonical AS relationship
LEFT JOIN
(
    SELECT
        address,
        entity_id,
        exchange_name,
        address_role,
        confidence
    FROM tron_db.exchange_addresses
    FINAL
    WHERE is_active = 1
) AS from_exchange ON from_exchange.address = relationship.from_address
LEFT JOIN
(
    SELECT
        address,
        entity_id,
        exchange_name,
        address_role,
        confidence
    FROM tron_db.exchange_addresses
    FINAL
    WHERE is_active = 1
) AS to_exchange ON to_exchange.address = relationship.to_address
WHERE (from_exchange.address != '') OR (to_exchange.address != '')
;

CREATE VIEW IF NOT EXISTS tron_db.transactions_canonical
(
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `initiator_address` String,
    `target_address` String,
    `contract_address` String,
    `contract_type` String,
    `fee` UInt256,
    `energy_usage_total` UInt64,
    `net_usage` UInt64,
    `status` UInt8,
    `inserted_at` DateTime64(3)
) AS
SELECT
    tx_hash,
    block_number,
    timestamp,
    initiator_address,
    target_address,
    contract_address,
    contract_type,
    fee,
    energy_usage_total,
    net_usage,
    status,
    inserted_at
FROM tron_db.transactions FINAL
INNER JOIN
(
    SELECT block_number
    FROM tron_db.ingested_blocks FINAL
    WHERE chain = 'tron' AND ingestion_status = 'COMPLETE'
) AS committed USING (block_number)
;

CREATE VIEW IF NOT EXISTS tron_db.wallet_asset_balances
(
    `address` String,
    `asset_type` LowCardinality(String),
    `asset_id` String,
    `asset_symbol` String,
    `asset_name` String,
    `decimals` UInt8,
    `metadata_verified` UInt8,
    `balance_raw` UInt256,
    `balance_incomplete` UInt8,
    `balance_decimal` Float64
) AS
WITH latest_metadata AS
    (
        SELECT
            token_address,
            name,
            symbol,
            decimals,
            is_verified
        FROM tron_db.token_metadata FINAL
    )
SELECT
    balances.address,
    balances.asset_type,
    balances.asset_id,
    multiIf(balances.asset_type = 'native', 'TRX', balances.asset_type = 'trc10', balances.asset_id, latest_metadata.symbol = '', balances.asset_id, latest_metadata.symbol) AS asset_symbol,
    multiIf(balances.asset_type = 'native', 'TRON', balances.asset_type = 'trc10', '', latest_metadata.name) AS asset_name,
    multiIf(balances.asset_type = 'native', toUInt8(6), balances.asset_type = 'trc10', toUInt8(0), latest_metadata.decimals) AS decimals,
    multiIf(balances.asset_type = 'native', toUInt8(1), balances.asset_type = 'trc10', toUInt8(0), latest_metadata.is_verified) AS metadata_verified,
    balances.balance_raw,
    balances.balance_incomplete,
    if(balances.asset_type = 'trc10', toFloat64(balances.balance_raw), toFloat64(balances.balance_raw) / pow(10, if(balances.asset_type = 'native', toUInt8(6), latest_metadata.decimals))) AS balance_decimal
FROM
(
    SELECT
        address,
        asset_type,
        asset_id,
        toUInt256(if(sumIf(amount_raw, direction = 1) >= sumIf(amount_raw, direction = -1), sumIf(amount_raw, direction = 1) - sumIf(amount_raw, direction = -1), toInt256(0))) AS balance_raw,
        toUInt8(sumIf(amount_raw, direction = 1) < sumIf(amount_raw, direction = -1)) AS balance_incomplete
    FROM tron_db.wallet_asset_balance_deltas_v3
    FINAL
    INNER JOIN
    (
        SELECT block_number
        FROM tron_db.ingested_blocks FINAL
        WHERE chain = 'tron' AND ingestion_status = 'COMPLETE'
    ) AS committed USING (block_number)
    GROUP BY
        address,
        asset_type,
        asset_id
    HAVING (balance_raw > 0) OR (balance_incomplete = 1)
) AS balances
LEFT JOIN latest_metadata ON (balances.asset_type = 'trc20') AND (balances.asset_id = latest_metadata.token_address)
;

CREATE VIEW IF NOT EXISTS tron_db.semantic_aml_events_canonical
(
    `event_id` String,
    `chain` LowCardinality(String),
    `tx_hash` String,
    `block_number` UInt64,
    `timestamp` UInt64,
    `event_type` LowCardinality(String),
    `subject_address` String,
    `protocol` String,
    `asset_in` String,
    `asset_out` String,
    `detector` String,
    `detector_version` String,
    `confidence` Float32,
    `evidence_json` String,
    `inserted_at` DateTime64(3)
) AS
SELECT event_id, chain, tx_hash, block_number, timestamp, event_type,
       subject_address, protocol, asset_in, asset_out, detector,
       detector_version, confidence, evidence_json, inserted_at
FROM tron_db.semantic_aml_events FINAL
INNER JOIN
(
    SELECT block_number
    FROM tron_db.ingested_blocks FINAL
    WHERE chain = 'tron' AND ingestion_status = 'COMPLETE'
) AS committed USING (block_number)
;
