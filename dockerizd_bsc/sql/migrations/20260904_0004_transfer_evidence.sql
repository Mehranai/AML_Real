-- Bind token discovery evidence to the same complete block revision as its source log.

ALTER TABLE {{database}}.token_metadata_discoveries
    ADD COLUMN IF NOT EXISTS block_hash String AFTER discovered_block;
ALTER TABLE {{database}}.token_metadata_discoveries
    ADD COLUMN IF NOT EXISTS block_state_revision UInt64 AFTER block_hash;

CREATE VIEW IF NOT EXISTS {{database}}.token_metadata_discoveries_canonical AS
SELECT fact.*
FROM {{database}}.token_metadata_discoveries AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.discovered_block = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision
ORDER BY fact.discovered_block ASC, fact.inserted_at ASC
LIMIT 1 BY fact.network_id, fact.token_address, fact.standard_hint;
