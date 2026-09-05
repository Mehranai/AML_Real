-- Ethereum AML canonical evidence schema.
-- This migration is immutable after it has been applied.

CREATE TABLE IF NOT EXISTS {{database}}.ingested_blocks
(
    network_id LowCardinality(String),
    block_number UInt64,
    block_hash String,
    parent_hash String,
    block_timestamp_unix_ms UInt64,
    transaction_count UInt32,
    ingestion_status LowCardinality(String),
    error_message String CODEC(ZSTD(3)),
    indexed_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, block_number);

CREATE TABLE IF NOT EXISTS {{database}}.transactions
(
    network_id LowCardinality(String),
    tx_hash String,
    block_hash String,
    block_number UInt64,
    block_timestamp_unix_ms UInt64,
    transaction_index UInt32,
    from_address String,
    to_address String,
    contract_address String DEFAULT '',
    nonce UInt64,
    transaction_type UInt8,
    value UInt256,
    input_selector String DEFAULT '',
    input_data String CODEC(ZSTD(3)),
    status UInt8,
    status_known UInt8,
    gas_limit UInt64,
    gas_used UInt64,
    effective_gas_price UInt256,
    fee_paid UInt256,
    max_fee_per_gas UInt256,
    max_priority_fee_per_gas UInt256,
    blob_gas_used UInt64,
    blob_gas_price UInt256,
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (block_number, transaction_index, tx_hash);

ALTER TABLE {{database}}.transactions
    ADD INDEX IF NOT EXISTS idx_transactions_hash tx_hash TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.transactions
    ADD INDEX IF NOT EXISTS idx_transactions_from from_address TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.transactions
    ADD INDEX IF NOT EXISTS idx_transactions_to to_address TYPE bloom_filter(0.001) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.evm_logs
(
    event_id String,
    network_id LowCardinality(String),
    block_hash String,
    block_number UInt64,
    block_timestamp_unix_ms UInt64,
    tx_hash String,
    transaction_index UInt32,
    log_index UInt32,
    contract_address String,
    topic0 String DEFAULT '',
    topics Array(String),
    data String CODEC(ZSTD(3)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (block_number, transaction_index, log_index, event_id);

ALTER TABLE {{database}}.evm_logs
    ADD INDEX IF NOT EXISTS idx_logs_tx tx_hash TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.evm_logs
    ADD INDEX IF NOT EXISTS idx_logs_contract contract_address TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.evm_logs
    ADD INDEX IF NOT EXISTS idx_logs_topic0 topic0 TYPE bloom_filter(0.001) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.address_relationships
(
    relationship_id String,
    network_id LowCardinality(String),
    block_hash String,
    block_number UInt64,
    block_timestamp_unix_ms UInt64,
    tx_hash String DEFAULT '',
    transaction_index UInt32,
    event_index UInt32,
    trace_address Array(UInt32),
    from_address String,
    to_address String,
    asset_id String,
    token_id String DEFAULT '',
    amount UInt256,
    transfer_type LowCardinality(String),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY relationship_id;

ALTER TABLE {{database}}.address_relationships
    ADD INDEX IF NOT EXISTS idx_relationship_from from_address TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.address_relationships
    ADD INDEX IF NOT EXISTS idx_relationship_to to_address TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.address_relationships
    ADD INDEX IF NOT EXISTS idx_relationship_tx tx_hash TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.address_relationships
    ADD INDEX IF NOT EXISTS idx_relationship_asset asset_id TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.address_relationships
    ADD INDEX IF NOT EXISTS idx_relationship_type transfer_type TYPE set(32) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.transaction_features
(
    feature_id String,
    network_id LowCardinality(String),
    tx_hash String,
    block_number UInt64,
    block_timestamp_unix_ms UInt64,
    transaction_type LowCardinality(String),
    transaction_subtype LowCardinality(String),
    protocol String,
    method_id String,
    is_swap UInt8,
    is_bridge UInt8,
    is_mint UInt8,
    is_burn UInt8,
    is_liquidity_add UInt8,
    is_liquidity_remove UInt8,
    is_contract_call UInt8,
    unique_assets UInt16,
    participants UInt16,
    classification_confidence Float32,
    detector String,
    detector_version String,
    evidence_refs Array(String),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY feature_id;

ALTER TABLE {{database}}.transaction_features
    ADD INDEX IF NOT EXISTS idx_features_tx tx_hash TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.transaction_features
    ADD INDEX IF NOT EXISTS idx_features_type transaction_type TYPE set(100) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.semantic_aml_events
(
    event_id String,
    network_id LowCardinality(String),
    tx_hash String,
    block_number UInt64,
    block_timestamp_unix_ms UInt64,
    event_type LowCardinality(String),
    subject_address String,
    protocol String,
    asset_in String DEFAULT '',
    asset_out String DEFAULT '',
    amount_in String DEFAULT '',
    amount_out String DEFAULT '',
    detector String,
    detector_version String,
    confidence Float32,
    evidence_json String CODEC(ZSTD(3)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY event_id;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_subject subject_address TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_type event_type TYPE set(100) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.token_metadata
(
    network_id LowCardinality(String),
    token_address String,
    token_standard LowCardinality(String),
    name String,
    symbol String,
    decimals UInt8,
    total_supply String,
    code_hash String DEFAULT '',
    is_verified UInt8,
    metadata_source String,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, token_address);

CREATE TABLE IF NOT EXISTS {{database}}.token_metadata_discoveries
(
    network_id LowCardinality(String),
    token_address String,
    discovered_block UInt64,
    discovered_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, token_address);

CREATE TABLE IF NOT EXISTS {{database}}.token_metadata_jobs
(
    network_id LowCardinality(String),
    token_address String,
    discovered_block UInt64,
    status LowCardinality(String),
    attempt_count UInt8,
    last_error String CODEC(ZSTD(3)),
    updated_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, token_address);

CREATE TABLE IF NOT EXISTS {{database}}.ingestion_failures
(
    failure_id String,
    network_id LowCardinality(String),
    block_number UInt64,
    block_hash String,
    tx_hash String,
    stage LowCardinality(String),
    error_class LowCardinality(String),
    error_message String CODEC(ZSTD(3)),
    retryable UInt8,
    attempt_count UInt32,
    status LowCardinality(String),
    first_failed_at_unix_ms UInt64,
    last_failed_at_unix_ms UInt64,
    resolved_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, failure_id);

ALTER TABLE {{database}}.ingestion_failures
    ADD INDEX IF NOT EXISTS idx_failure_status status TYPE set(16) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.sync_state
(
    network_id LowCardinality(String),
    last_synced_block UInt64,
    last_synced_block_hash String,
    updated_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY network_id;

CREATE TABLE IF NOT EXISTS {{database}}.ingestion_benchmarks
(
    benchmark_id String,
    network_id LowCardinality(String),
    start_block UInt64,
    end_block UInt64,
    completed_blocks UInt64,
    transaction_count UInt64,
    transfer_count UInt64,
    elapsed_ms UInt64,
    transactions_per_second Float64,
    compressed_bytes UInt64,
    active_parts UInt64,
    created_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree
ORDER BY (network_id, created_at, benchmark_id)
TTL toDateTime(created_at) + INTERVAL 365 DAY;

CREATE VIEW IF NOT EXISTS {{database}}.transactions_canonical AS
SELECT *
FROM {{database}}.transactions
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, tx_hash;

CREATE VIEW IF NOT EXISTS {{database}}.evm_logs_canonical AS
SELECT *
FROM {{database}}.evm_logs
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, event_id;

CREATE VIEW IF NOT EXISTS {{database}}.address_relationships_canonical AS
SELECT *
FROM {{database}}.address_relationships
WHERE amount > 0
  AND from_address != ''
  AND to_address != ''
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, relationship_id;

CREATE VIEW IF NOT EXISTS {{database}}.transaction_features_canonical AS
SELECT *
FROM {{database}}.transaction_features
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, feature_id;

CREATE VIEW IF NOT EXISTS {{database}}.semantic_aml_events_canonical AS
SELECT *
FROM {{database}}.semantic_aml_events
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, event_id;
