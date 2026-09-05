-- BSC AML core evidence and operational state.
-- Immutable after its checksum is recorded in schema_migrations.

CREATE TABLE IF NOT EXISTS {{database}}.ingested_blocks
(
    network_id LowCardinality(String),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_hash String CODEC(ZSTD(1)),
    parent_hash String CODEC(ZSTD(1)),
    block_timestamp_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    transaction_count UInt32,
    log_count UInt32,
    receipt_data_complete UInt8,
    trace_data_complete UInt8,
    canonical UInt8,
    ingestion_status LowCardinality(String),
    rpc_provider LowCardinality(String),
    rpc_client_version String CODEC(ZSTD(1)),
    state_revision UInt64,
    indexed_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(state_revision)
ORDER BY (network_id, block_number)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.transactions
(
    network_id LowCardinality(String),
    tx_hash String CODEC(ZSTD(1)),
    block_hash String CODEC(ZSTD(1)),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_timestamp_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    transaction_index UInt32,
    from_address String CODEC(ZSTD(1)),
    to_address String CODEC(ZSTD(1)),
    contract_address String DEFAULT '' CODEC(ZSTD(1)),
    nonce UInt64,
    transaction_type UInt8,
    value UInt256 CODEC(ZSTD(1)),
    input_selector String DEFAULT '',
    input_data String CODEC(ZSTD(3)),
    status UInt8,
    status_known UInt8,
    gas_limit UInt64,
    gas_used UInt64,
    effective_gas_price UInt256 CODEC(ZSTD(1)),
    fee_paid UInt256 CODEC(ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (network_id, block_number, transaction_index, tx_hash, block_hash)
SETTINGS index_granularity = 8192;

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
    block_hash String CODEC(ZSTD(1)),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_timestamp_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    tx_hash String CODEC(ZSTD(1)),
    transaction_index UInt32,
    log_index UInt32,
    contract_address String CODEC(ZSTD(1)),
    topic0 String DEFAULT '' CODEC(ZSTD(1)),
    topics Array(String) CODEC(ZSTD(3)),
    data String CODEC(ZSTD(3)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (network_id, block_number, transaction_index, log_index, event_id, block_hash)
SETTINGS index_granularity = 8192;

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
    block_hash String CODEC(ZSTD(1)),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_timestamp_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    tx_hash String CODEC(ZSTD(1)),
    transaction_index UInt32,
    event_index UInt32,
    event_sub_index UInt32,
    trace_address Array(UInt32) CODEC(ZSTD(1)),
    from_address String CODEC(ZSTD(1)),
    to_address String CODEC(ZSTD(1)),
    asset_id String CODEC(ZSTD(1)),
    token_id String DEFAULT '' CODEC(ZSTD(1)),
    amount UInt256 CODEC(ZSTD(1)),
    transfer_type LowCardinality(String),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (network_id, block_number, transaction_index, relationship_id, block_hash)
SETTINGS index_granularity = 8192;

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
    block_hash String CODEC(ZSTD(1)),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_timestamp_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    tx_hash String CODEC(ZSTD(1)),
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
    classification_source LowCardinality(String),
    detector String,
    detector_version String,
    evidence_refs Array(String) CODEC(ZSTD(3)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (network_id, block_number, feature_id, block_hash)
SETTINGS index_granularity = 8192;

ALTER TABLE {{database}}.transaction_features
    ADD INDEX IF NOT EXISTS idx_features_tx tx_hash TYPE bloom_filter(0.001) GRANULARITY 4;
ALTER TABLE {{database}}.transaction_features
    ADD INDEX IF NOT EXISTS idx_features_type transaction_type TYPE set(100) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.semantic_aml_events
(
    event_id String,
    network_id LowCardinality(String),
    block_hash String CODEC(ZSTD(1)),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_timestamp_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    tx_hash String CODEC(ZSTD(1)),
    event_type LowCardinality(String),
    subject_address String CODEC(ZSTD(1)),
    protocol String,
    protocol_contract String CODEC(ZSTD(1)),
    correlation_key String DEFAULT '' CODEC(ZSTD(1)),
    asset_in String DEFAULT '' CODEC(ZSTD(1)),
    asset_out String DEFAULT '' CODEC(ZSTD(1)),
    amount_in String DEFAULT '' CODEC(ZSTD(1)),
    amount_out String DEFAULT '' CODEC(ZSTD(1)),
    detector String,
    detector_version String,
    confidence Float32,
    evidence_refs Array(String) CODEC(ZSTD(3)),
    evidence_json String CODEC(ZSTD(3)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(block_timestamp_unix_ms, 1000)))
ORDER BY (network_id, block_number, event_id, block_hash)
SETTINGS index_granularity = 8192;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_subject subject_address TYPE bloom_filter(0.001) GRANULARITY 4;
ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_type event_type TYPE set(100) GRANULARITY 4;
ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_tx tx_hash TYPE bloom_filter(0.001) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.token_metadata
(
    network_id LowCardinality(String),
    token_address String CODEC(ZSTD(1)),
    token_standard LowCardinality(String),
    name String,
    symbol String,
    decimals Nullable(UInt8),
    metadata_status LowCardinality(String),
    metadata_source LowCardinality(String),
    is_verified UInt8,
    observed_block UInt64 CODEC(Delta, ZSTD(1)),
    created_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, token_address)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.token_metadata_discoveries
(
    discovery_id String,
    network_id LowCardinality(String),
    token_address String CODEC(ZSTD(1)),
    standard_hint LowCardinality(String),
    discovered_block UInt64 CODEC(Delta, ZSTD(1)),
    tx_hash String CODEC(ZSTD(1)),
    evidence_id String,
    created_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, token_address, discovery_id)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.token_metadata_jobs
(
    network_id LowCardinality(String),
    token_address String CODEC(ZSTD(1)),
    status LowCardinality(String),
    attempt_count UInt16,
    last_error_class LowCardinality(String),
    next_attempt_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    updated_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, token_address)
SETTINGS index_granularity = 8192;

ALTER TABLE {{database}}.token_metadata_jobs
    ADD INDEX IF NOT EXISTS idx_metadata_job_status status TYPE set(16) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.sync_state
(
    network_id LowCardinality(String),
    next_block UInt64,
    last_finalized_block UInt64,
    last_finalized_block_hash String CODEC(ZSTD(1)),
    state_revision UInt64,
    updated_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(state_revision)
ORDER BY network_id
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS {{database}}.ingestion_failures
(
    failure_id String,
    network_id LowCardinality(String),
    block_number UInt64 CODEC(Delta, ZSTD(1)),
    block_hash String CODEC(ZSTD(1)),
    tx_hash String CODEC(ZSTD(1)),
    stage LowCardinality(String),
    error_class LowCardinality(String),
    error_summary String CODEC(ZSTD(3)),
    retryable UInt8,
    attempt_count UInt32,
    status LowCardinality(String),
    created_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    updated_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    resolved_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, failure_id)
TTL toDateTime(intDiv(updated_at_unix_ms, 1000)) + INTERVAL 180 DAY DELETE WHERE status = 'resolved'
SETTINGS index_granularity = 8192;

ALTER TABLE {{database}}.ingestion_failures
    ADD INDEX IF NOT EXISTS idx_failure_status status TYPE set(16) GRANULARITY 4;
ALTER TABLE {{database}}.ingestion_failures
    ADD INDEX IF NOT EXISTS idx_failure_block block_number TYPE minmax GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.ingestion_benchmarks
(
    benchmark_id String,
    network_id LowCardinality(String),
    start_block UInt64,
    end_block UInt64,
    completed_blocks UInt64,
    transaction_count UInt64,
    log_count UInt64,
    relationship_count UInt64,
    elapsed_ms UInt64,
    rows_per_second Float64,
    compressed_bytes UInt64,
    uncompressed_bytes UInt64,
    created_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree
ORDER BY (network_id, created_at, benchmark_id)
TTL toDateTime(created_at) + INTERVAL 365 DAY
SETTINGS index_granularity = 8192;
