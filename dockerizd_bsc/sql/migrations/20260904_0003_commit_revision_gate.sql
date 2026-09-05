-- Bind every block-scoped fact to the exact block commit that produced it.
-- This prevents staged rows from an interrupted attempt becoming visible after a retry.

DROP VIEW IF EXISTS {{database}}.transactions_canonical;
DROP VIEW IF EXISTS {{database}}.evm_logs_canonical;
DROP VIEW IF EXISTS {{database}}.address_relationships_canonical;
DROP VIEW IF EXISTS {{database}}.transaction_features_canonical;
DROP VIEW IF EXISTS {{database}}.semantic_aml_events_canonical;

ALTER TABLE {{database}}.transactions
    ADD COLUMN IF NOT EXISTS block_state_revision UInt64 AFTER block_timestamp_unix_ms;
ALTER TABLE {{database}}.transactions
    DROP COLUMN IF EXISTS status_known;

ALTER TABLE {{database}}.evm_logs
    ADD COLUMN IF NOT EXISTS block_state_revision UInt64 AFTER block_timestamp_unix_ms;

ALTER TABLE {{database}}.address_relationships
    ADD COLUMN IF NOT EXISTS block_state_revision UInt64 AFTER block_timestamp_unix_ms;

ALTER TABLE {{database}}.transaction_features
    ADD COLUMN IF NOT EXISTS block_state_revision UInt64 AFTER block_timestamp_unix_ms;

ALTER TABLE {{database}}.semantic_aml_events
    ADD COLUMN IF NOT EXISTS block_state_revision UInt64 AFTER block_timestamp_unix_ms;

CREATE VIEW {{database}}.transactions_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.transactions
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, tx_hash, block_hash, block_state_revision
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision;

CREATE VIEW {{database}}.evm_logs_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.evm_logs
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, event_id, block_hash, block_state_revision
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision;

CREATE VIEW {{database}}.address_relationships_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.address_relationships
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, relationship_id, block_hash, block_state_revision
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision
WHERE fact.amount > 0
  AND fact.from_address != ''
  AND fact.to_address != '';

CREATE VIEW {{database}}.transaction_features_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.transaction_features
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, feature_id, block_hash, block_state_revision
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision;

CREATE VIEW {{database}}.semantic_aml_events_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.semantic_aml_events
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, event_id, block_hash, block_state_revision
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision;
