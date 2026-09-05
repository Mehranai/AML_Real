-- Reviewed clustering evidence and explainable Ethereum exposure propagation.

ALTER TABLE {{database}}.entity_labels
    ADD COLUMN IF NOT EXISTS is_exposure_seed UInt8 DEFAULT 0 AFTER risk_level;

ALTER TABLE {{database}}.entity_labels
    ADD COLUMN IF NOT EXISTS seed_category LowCardinality(String) DEFAULT '' AFTER is_exposure_seed;

ALTER TABLE {{database}}.address_entities
    ADD COLUMN IF NOT EXISTS is_exposure_seed UInt8 DEFAULT 0 AFTER risk_level;

ALTER TABLE {{database}}.address_entities
    ADD COLUMN IF NOT EXISTS seed_category LowCardinality(String) DEFAULT '' AFTER is_exposure_seed;

DROP VIEW IF EXISTS {{database}}.address_entities_active;

CREATE VIEW {{database}}.address_entities_active AS
SELECT *
FROM {{database}}.address_entities FINAL
WHERE is_active = 1;

CREATE TABLE IF NOT EXISTS {{database}}.cluster_runs
(
    run_id String,
    network_id LowCardinality(String),
    detector_version String,
    status LowCardinality(String),
    entity_memberships UInt64,
    control_claims UInt64,
    started_at_unix_ms UInt64,
    completed_at_unix_ms UInt64,
    error_message String DEFAULT '',
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, run_id);

CREATE TABLE IF NOT EXISTS {{database}}.address_cluster_claims
(
    claim_id String,
    run_id String,
    network_id LowCardinality(String),
    left_address String,
    right_address String,
    cluster_id String DEFAULT '',
    claim_type LowCardinality(String),
    same_entity UInt8,
    confidence Float32,
    review_status LowCardinality(String),
    source_id String,
    evidence_tx_hash String DEFAULT '',
    evidence_refs Array(String),
    detector String,
    detector_version String,
    evidence_json String CODEC(ZSTD(3)),
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_cluster_claim_left left_address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_cluster_claim_right right_address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, claim_id);

CREATE TABLE IF NOT EXISTS {{database}}.address_cluster_memberships
(
    membership_id String,
    run_id String,
    network_id LowCardinality(String),
    address String,
    cluster_id String,
    entity_id String,
    membership_type LowCardinality(String),
    confidence Float32,
    source_claim_id String,
    is_active UInt8,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_cluster_member_address address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_cluster_member_cluster cluster_id TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, address, cluster_id, membership_id);

CREATE VIEW IF NOT EXISTS {{database}}.address_cluster_memberships_active AS
SELECT *
FROM {{database}}.address_cluster_memberships FINAL
WHERE is_active = 1;

CREATE VIEW IF NOT EXISTS {{database}}.service_boundary_addresses AS
SELECT
    network_id,
    address,
    entity_id AS service_entity_id,
    entity_type AS service_type
FROM {{database}}.address_entities_active
WHERE entity_type IN ('exchange', 'custodian', 'bridge', 'mixer', 'dex')
   OR address_role IN ('HOT_WALLET', 'DEPOSIT', 'BRIDGE', 'MIXER', 'POOL', 'ROUTER')
UNION ALL
SELECT
    network_id,
    contract_address AS address,
    concat('protocol:', protocol) AS service_entity_id,
    protocol_type AS service_type
FROM {{database}}.protocol_contract_registry_active
WHERE protocol_type IN ('exchange', 'custodian', 'bridge', 'mixer', 'dex', 'swap', 'liquidity')
   OR contract_role IN ('HOT_WALLET', 'DEPOSIT', 'BRIDGE', 'MIXER', 'POOL', 'ROUTER');

CREATE TABLE IF NOT EXISTS {{database}}.exposure_runs
(
    run_id String,
    network_id LowCardinality(String),
    detector_version String,
    status LowCardinality(String),
    max_hops UInt8,
    hop_decay Float64,
    time_half_life_days Float64,
    max_paths_per_subject UInt16,
    seed_count UInt64,
    path_count UInt64,
    started_at_unix_ms UInt64,
    completed_at_unix_ms UInt64,
    error_message String DEFAULT '',
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, run_id);

CREATE TABLE IF NOT EXISTS {{database}}.address_exposure_paths
(
    path_id String,
    run_id String,
    network_id LowCardinality(String),
    subject_address String,
    seed_address String,
    seed_entity_id String,
    seed_category LowCardinality(String),
    seed_risk_level UInt8,
    direction LowCardinality(String),
    hop_count UInt8,
    asset_id String,
    exposure_score Float64,
    amount_share Float64,
    time_weight Float64,
    service_mediated UInt8,
    service_entity_id String DEFAULT '',
    continuity_type LowCardinality(String),
    path_addresses Array(String),
    relationship_ids Array(String),
    tx_hashes Array(String),
    first_transfer_unix_ms UInt64,
    last_transfer_unix_ms UInt64,
    detector_version String,
    created_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_exposure_subject subject_address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_exposure_seed seed_address TYPE bloom_filter(0.001) GRANULARITY 4,
    INDEX idx_exposure_run run_id TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY tuple(network_id, toYYYYMM(toDateTime(intDiv(created_at_unix_ms, 1000))))
ORDER BY (network_id, run_id, hop_count, seed_address, subject_address, path_id)
TTL toDateTime(intDiv(created_at_unix_ms, 1000)) + INTERVAL 180 DAY;

CREATE VIEW IF NOT EXISTS {{database}}.address_exposure_best_paths AS
SELECT *
FROM {{database}}.address_exposure_paths
ORDER BY exposure_score DESC, hop_count ASC, last_transfer_unix_ms DESC
LIMIT 1 BY network_id, run_id, subject_address, seed_address, direction;
