-- ClickHouse 23.8 freezes SELECT * view columns at creation time. Recreate the
-- affected views after migration 0003 added semantic evidence columns.

DROP VIEW IF EXISTS {{database}}.address_relationships_canonical;

CREATE VIEW {{database}}.address_relationships_canonical AS
SELECT *
FROM {{database}}.address_relationships
WHERE amount > 0
  AND from_address != ''
  AND to_address != ''
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, relationship_id;

DROP VIEW IF EXISTS {{database}}.semantic_aml_events_canonical;

CREATE VIEW {{database}}.semantic_aml_events_canonical AS
SELECT *
FROM {{database}}.semantic_aml_events
ORDER BY inserted_at DESC
LIMIT 1 BY network_id, event_id;

DROP VIEW IF EXISTS {{database}}.protocol_contract_registry_active;

CREATE VIEW {{database}}.protocol_contract_registry_active AS
SELECT *
FROM {{database}}.protocol_contract_registry FINAL
WHERE enabled = 1;
