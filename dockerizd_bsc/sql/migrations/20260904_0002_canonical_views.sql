-- Canonical/current read contracts.
-- Every block-scoped fact is joined to the latest complete canonical block hash.

CREATE VIEW IF NOT EXISTS {{database}}.ingested_blocks_canonical AS
SELECT
    network_id,
    block_number,
    argMax(block_hash, state_revision) AS block_hash,
    argMax(parent_hash, state_revision) AS parent_hash,
    argMax(block_timestamp_unix_ms, state_revision) AS block_timestamp_unix_ms,
    argMax(transaction_count, state_revision) AS transaction_count,
    argMax(log_count, state_revision) AS log_count,
    argMax(receipt_data_complete, state_revision) AS receipt_data_complete,
    argMax(trace_data_complete, state_revision) AS trace_data_complete,
    argMax(rpc_provider, state_revision) AS rpc_provider,
    argMax(rpc_client_version, state_revision) AS rpc_client_version,
    max(state_revision) AS current_revision,
    argMax(indexed_at_unix_ms, state_revision) AS indexed_at_unix_ms
FROM {{database}}.ingested_blocks
GROUP BY network_id, block_number
HAVING argMax(canonical, state_revision) = 1
   AND argMax(ingestion_status, state_revision) = 'complete';

CREATE VIEW IF NOT EXISTS {{database}}.transactions_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.transactions
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, tx_hash, block_hash
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash;

CREATE VIEW IF NOT EXISTS {{database}}.evm_logs_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.evm_logs
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, event_id, block_hash
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash;

CREATE VIEW IF NOT EXISTS {{database}}.address_relationships_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.address_relationships
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, relationship_id, block_hash
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
WHERE fact.amount > 0
  AND fact.from_address != ''
  AND fact.to_address != '';

CREATE VIEW IF NOT EXISTS {{database}}.transaction_features_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.transaction_features
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, feature_id, block_hash
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash;

CREATE VIEW IF NOT EXISTS {{database}}.semantic_aml_events_canonical AS
SELECT fact.*
FROM
(
    SELECT *
    FROM {{database}}.semantic_aml_events
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, event_id, block_hash
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash;

CREATE VIEW IF NOT EXISTS {{database}}.token_metadata_current AS
SELECT *
FROM {{database}}.token_metadata
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, token_address;

CREATE VIEW IF NOT EXISTS {{database}}.token_metadata_jobs_current AS
SELECT *
FROM {{database}}.token_metadata_jobs
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, token_address;

CREATE VIEW IF NOT EXISTS {{database}}.sync_state_current AS
SELECT *
FROM {{database}}.sync_state
ORDER BY state_revision DESC
LIMIT 1 BY network_id;

CREATE VIEW IF NOT EXISTS {{database}}.ingestion_failures_current AS
SELECT *
FROM {{database}}.ingestion_failures
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, failure_id;
