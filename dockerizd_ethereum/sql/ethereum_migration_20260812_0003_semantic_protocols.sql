-- Extend canonical evidence with ERC-1155 item identity and protocol semantics.
-- This migration is immutable after it has been applied.

ALTER TABLE {{database}}.address_relationships
    ADD COLUMN IF NOT EXISTS event_sub_index UInt32 DEFAULT 0 AFTER event_index;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS event_index UInt32 DEFAULT 0 AFTER block_timestamp_unix_ms;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS protocol_contract String DEFAULT '' AFTER protocol;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS counterparty_address String DEFAULT '' AFTER protocol_contract;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS remote_network_id LowCardinality(String) DEFAULT '' AFTER asset_out;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS remote_asset String DEFAULT '' AFTER remote_network_id;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS bridge_direction LowCardinality(String) DEFAULT '' AFTER remote_asset;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS correlation_key String DEFAULT '' AFTER bridge_direction;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_protocol_contract protocol_contract TYPE bloom_filter(0.001) GRANULARITY 4;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_remote_network remote_network_id TYPE set(100) GRANULARITY 4;

ALTER TABLE {{database}}.semantic_aml_events
    ADD INDEX IF NOT EXISTS idx_semantic_correlation correlation_key TYPE bloom_filter(0.001) GRANULARITY 4;

CREATE TABLE IF NOT EXISTS {{database}}.protocol_contract_registry
(
    network_id LowCardinality(String),
    contract_address String,
    protocol String,
    protocol_type LowCardinality(String),
    contract_role LowCardinality(String),
    remote_network_id LowCardinality(String) DEFAULT '',
    remote_contract_address String DEFAULT '',
    decoder LowCardinality(String),
    source String,
    confidence Float32,
    enabled UInt8,
    created_at_unix_ms UInt64,
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (network_id, contract_address);

ALTER TABLE {{database}}.protocol_contract_registry
    ADD INDEX IF NOT EXISTS idx_protocol_registry_type protocol_type TYPE set(32) GRANULARITY 1;

CREATE VIEW IF NOT EXISTS {{database}}.protocol_contract_registry_active AS
SELECT *
FROM {{database}}.protocol_contract_registry
WHERE enabled = 1
ORDER BY updated_at DESC
LIMIT 1 BY network_id, contract_address;

INSERT INTO {{database}}.protocol_contract_registry
(
    network_id,
    contract_address,
    protocol,
    protocol_type,
    contract_role,
    remote_network_id,
    remote_contract_address,
    decoder,
    source,
    confidence,
    enabled,
    created_at_unix_ms
)
VALUES
(
    'eip155:1',
    '0x99c9fc46f92e8a1c0dec1b1747d010903e884be1',
    'optimism_standard_bridge',
    'bridge',
    'l1_standard_bridge',
    'eip155:10',
    '0x4200000000000000000000000000000000000010',
    'op_standard_bridge_v1',
    'https://docs.optimism.io/app-developers/tutorials/bridging/deposit-transactions',
    1.0,
    1,
    1786492800000
);
