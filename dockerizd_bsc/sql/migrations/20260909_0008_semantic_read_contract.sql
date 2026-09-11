-- ClickHouse freezes SELECT * view columns at creation; expose the Phase 6 additions.
CREATE OR REPLACE VIEW {{database}}.semantic_aml_events_canonical AS
SELECT fact.*
FROM
(
    SELECT * FROM {{database}}.semantic_aml_events
    ORDER BY inserted_at DESC
    LIMIT 1 BY network_id, event_id, block_hash, block_state_revision
) AS fact
INNER JOIN {{database}}.ingested_blocks_canonical AS block
    ON fact.network_id = block.network_id
   AND fact.block_number = block.block_number
   AND fact.block_hash = block.block_hash
   AND fact.block_state_revision = block.current_revision;
