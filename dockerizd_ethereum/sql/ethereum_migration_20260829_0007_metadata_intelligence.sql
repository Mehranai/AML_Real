-- Token metadata completeness and reviewed Ethereum entity intelligence.

ALTER TABLE {{database}}.token_metadata
    ADD COLUMN IF NOT EXISTS decimals_known UInt8 DEFAULT 0 AFTER decimals;

ALTER TABLE {{database}}.token_metadata
    ADD COLUMN IF NOT EXISTS metadata_status LowCardinality(String) DEFAULT 'partial'
    AFTER metadata_source;

CREATE VIEW IF NOT EXISTS {{database}}.token_metadata_canonical AS
SELECT *
FROM {{database}}.token_metadata
ORDER BY updated_at DESC
LIMIT 1 BY network_id, token_address;

CREATE TABLE IF NOT EXISTS {{database}}.intelligence_sources
(
    network_id LowCardinality(String),
    source_id String,
    source_name String,
    source_type LowCardinality(String),
    trust_tier LowCardinality(String),
    reference_url String DEFAULT '',
    is_active UInt8,
    created_by String,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, source_id);

CREATE TABLE IF NOT EXISTS {{database}}.entity_labels
(
    label_id String,
    network_id LowCardinality(String),
    address String,
    entity_id String,
    entity_name String,
    entity_type LowCardinality(String),
    address_role LowCardinality(String),
    confidence Float32,
    risk_level UInt8 DEFAULT 0,
    source_id String,
    source_record_id String,
    supersedes_label_id String DEFAULT '',
    submitted_by String,
    case_id String DEFAULT '',
    evidence_refs Array(String),
    review_status LowCardinality(String),
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_entity_label_address address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_entity_label_entity entity_id TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, address, label_id);

CREATE TABLE IF NOT EXISTS {{database}}.intelligence_reviews
(
    review_id String,
    network_id LowCardinality(String),
    subject_type LowCardinality(String),
    subject_id String,
    decision LowCardinality(String),
    reviewer String,
    reason String,
    evidence_refs Array(String),
    created_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(inserted_at)
ORDER BY (network_id, subject_type, subject_id, review_id);

CREATE TABLE IF NOT EXISTS {{database}}.address_entities
(
    network_id LowCardinality(String),
    address String,
    entity_id String,
    entity_name String,
    entity_type LowCardinality(String),
    address_role LowCardinality(String),
    confidence Float32,
    risk_level UInt8,
    source_label_id String,
    review_id String,
    is_active UInt8,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_address_entity entity_id TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_address_entity_type entity_type TYPE set(100) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, address, entity_id);

CREATE VIEW IF NOT EXISTS {{database}}.address_entities_active AS
SELECT *
FROM {{database}}.address_entities FINAL
WHERE is_active = 1;
