-- Activate semantic v2 evidence and carry token standards into metadata jobs.

ALTER TABLE {{database}}.transaction_features
    ADD COLUMN IF NOT EXISTS is_mixer UInt8 DEFAULT 0 AFTER is_bridge;

ALTER TABLE {{database}}.token_metadata_discoveries
    ADD COLUMN IF NOT EXISTS token_standard LowCardinality(String) DEFAULT 'unknown'
    AFTER token_address;

ALTER TABLE {{database}}.token_metadata_jobs
    ADD COLUMN IF NOT EXISTS token_standard LowCardinality(String) DEFAULT 'unknown'
    AFTER token_address;

DROP VIEW IF EXISTS {{database}}.transaction_features_canonical;

CREATE VIEW {{database}}.transaction_features_canonical AS
SELECT *
FROM {{database}}.transaction_features
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, feature_id;

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
    '0x12d66f87a04a9e220cbfe5c899a8d260a3a0d1d3',
    'tornado_cash',
    'mixer',
    'eth_0_1_pool',
    '',
    '',
    'tornado_cash_v1',
    'curated_ethereum_protocol_registry',
    1.0,
    1,
    1787961600000
);
