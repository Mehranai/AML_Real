-- Versioned, explainable policy scoring without ML probability claims.

CREATE TABLE IF NOT EXISTS {{database}}.risk_policies
(
    network_id LowCardinality(String),
    policy_version String,
    policy_name String,
    medium_threshold Float64,
    high_threshold Float64,
    enabled UInt8,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, policy_version);

CREATE TABLE IF NOT EXISTS {{database}}.risk_policy_rules
(
    network_id LowCardinality(String),
    policy_version String,
    rule_id String,
    signal_type LowCardinality(String),
    max_contribution Float64,
    description String,
    enabled UInt8,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, policy_version, rule_id);

CREATE TABLE IF NOT EXISTS {{database}}.wallet_risk_signals
(
    signal_id String,
    assessment_id String,
    network_id LowCardinality(String),
    address String,
    policy_version String,
    signal_type LowCardinality(String),
    signal_strength Float64,
    contribution Float64,
    summary String,
    evidence_refs Array(String),
    evidence_json String CODEC(ZSTD(3)),
    as_of_block UInt64,
    created_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_risk_signal_address address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(created_at_unix_ms, 1000)))
ORDER BY (network_id, address, assessment_id, signal_id)
TTL toDateTime(intDiv(created_at_unix_ms, 1000)) + INTERVAL 730 DAY;

CREATE TABLE IF NOT EXISTS {{database}}.wallet_risk_assessments
(
    assessment_id String,
    network_id LowCardinality(String),
    address String,
    policy_version String,
    risk_score Float64,
    risk_level LowCardinality(String),
    signal_count UInt16,
    top_reasons Array(String),
    evidence_refs Array(String),
    exposure_run_id String DEFAULT '',
    as_of_block UInt64,
    as_of_unix_ms UInt64,
    scoring_method LowCardinality(String),
    created_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3),
    INDEX idx_risk_assessment_address address TYPE bloom_filter(0.001) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(inserted_at)
PARTITION BY toYYYYMM(toDateTime(intDiv(created_at_unix_ms, 1000)))
ORDER BY (network_id, address, policy_version, assessment_id)
TTL toDateTime(intDiv(created_at_unix_ms, 1000)) + INTERVAL 1825 DAY;

CREATE VIEW IF NOT EXISTS {{database}}.wallet_risk_assessments_latest AS
SELECT *
FROM {{database}}.wallet_risk_assessments
ORDER BY as_of_unix_ms DESC, inserted_at DESC
LIMIT 1 BY network_id, address, policy_version;

INSERT INTO {{database}}.risk_policies
(
    network_id, policy_version, policy_name, medium_threshold,
    high_threshold, enabled, created_at_unix_ms
)
SELECT
    'eip155:1', 'ethereum_evidence_policy_v1',
    'Ethereum explainable AML evidence policy v1',
    40.0, 70.0, 1, 1787961600000
WHERE NOT EXISTS
(
    SELECT 1 FROM {{database}}.risk_policies
    WHERE network_id = 'eip155:1'
      AND policy_version = 'ethereum_evidence_policy_v1'
);

INSERT INTO {{database}}.risk_policy_rules
(
    network_id, policy_version, rule_id, signal_type,
    max_contribution, description, enabled, created_at_unix_ms
)
SELECT *
FROM
(
    SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R001', 'CONFIRMED_ILLICIT_SEED', 100.0, 'Address is an approved exposure seed', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R002', 'DIRECT_RECEIVED_EXPOSURE', 45.0, 'Direct amount/time weighted receipt from a reviewed seed', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R003', 'DIRECT_SENT_EXPOSURE', 30.0, 'Direct amount/time weighted transfer toward a reviewed seed', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R004', 'INDIRECT_EXPOSURE', 25.0, 'Explainable indirect exposure within configured graph depth', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R005', 'MIXER_INTERACTION', 35.0, 'Interaction with a confirmed mixer contract', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R006', 'MIXER_BRIDGE_SEQUENCE', 25.0, 'Mixer and bridge events occurred within one hour', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R007', 'RAPID_SWAP_BRIDGE_SEQUENCE', 15.0, 'Swap and bridge events occurred within one hour', 1, 1787961600000
    UNION ALL SELECT 'eip155:1', 'ethereum_evidence_policy_v1', 'R008', 'RAPID_PASS_THROUGH', 20.0, 'Same-asset receipt and onward transfer occurred within ten minutes', 1, 1787961600000
)
WHERE tuple('eip155:1', 'ethereum_evidence_policy_v1') NOT IN
(
    SELECT tuple(network_id, policy_version)
    FROM {{database}}.risk_policy_rules
);
