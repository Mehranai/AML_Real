-- Reviewed protocol intelligence and revision-gated semantic evidence.
-- Registry history is append-only; current/active views expose the latest review decision.

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS event_index UInt32 AFTER tx_hash;
ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS counterparty_address String DEFAULT '' AFTER protocol_contract;
ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS remote_network_id String DEFAULT '' AFTER asset_out;
ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS remote_asset String DEFAULT '' AFTER remote_network_id;
ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS bridge_direction LowCardinality(String) DEFAULT '' AFTER remote_asset;
ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS remote_receiver String DEFAULT '' AFTER bridge_direction;
ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS bridge_message_id String DEFAULT '' AFTER remote_receiver;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_protocol_contract protocol_contract TYPE bloom_filter(0.001) GRANULARITY 4;
ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_remote_network remote_network_id TYPE set(100) GRANULARITY 4;
ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_correlation correlation_key TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.ingestion_benchmarks
    ADD COLUMN IF NOT EXISTS feature_count UInt64 AFTER relationship_count;
ALTER TABLE {{database}}.ingestion_benchmarks
    ADD COLUMN IF NOT EXISTS semantic_event_count UInt64 AFTER feature_count;

CREATE TABLE IF NOT EXISTS {{database}}.protocol_contract_registry
(
    network_id LowCardinality(String),
    contract_address String CODEC(ZSTD(1)),
    protocol String,
    protocol_type LowCardinality(String),
    contract_role LowCardinality(String),
    decoder LowCardinality(String),
    remote_network_id String DEFAULT '',
    remote_contract_address String DEFAULT '' CODEC(ZSTD(1)),
    method_ids Array(String) CODEC(ZSTD(3)),
    method_event_types Array(String) CODEC(ZSTD(3)),
    event_topics Array(String) CODEC(ZSTD(3)),
    event_types Array(String) CODEC(ZSTD(3)),
    remote_receiver_topic_index Int8 DEFAULT -1,
    message_topic_index Int8 DEFAULT -1,
    source_id String,
    source_reference String CODEC(ZSTD(3)),
    review_status LowCardinality(String),
    evidence_confidence Float32,
    enabled UInt8,
    registry_revision UInt64,
    reviewed_by String,
    review_note String CODEC(ZSTD(3)),
    created_at_unix_ms UInt64 CODEC(Delta, ZSTD(1)),
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree
ORDER BY (network_id, contract_address, registry_revision)
SETTINGS index_granularity = 8192;

ALTER TABLE {{database}}.protocol_contract_registry
    ADD INDEX IF NOT EXISTS idx_protocol_registry_type protocol_type TYPE set(32) GRANULARITY 1;
ALTER TABLE {{database}}.protocol_contract_registry
    ADD INDEX IF NOT EXISTS idx_protocol_registry_review review_status TYPE set(8) GRANULARITY 1;

CREATE VIEW IF NOT EXISTS {{database}}.protocol_contract_registry_current AS
SELECT *
FROM {{database}}.protocol_contract_registry
ORDER BY registry_revision DESC, inserted_at DESC
LIMIT 1 BY network_id, contract_address;

CREATE VIEW IF NOT EXISTS {{database}}.protocol_contract_registry_active AS
SELECT *
FROM {{database}}.protocol_contract_registry_current
WHERE review_status = 'approved' AND enabled = 1;
