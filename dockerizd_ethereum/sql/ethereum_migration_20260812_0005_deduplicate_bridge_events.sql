-- OP Standard Bridge can emit legacy and current ABI events for one transfer.
-- Preserve both raw detector rows, but count the transfer once canonically.

DROP VIEW IF EXISTS {{database}}.semantic_aml_events_canonical;

CREATE VIEW {{database}}.semantic_aml_events_canonical AS
SELECT *
FROM {{database}}.semantic_aml_events
ORDER BY inserted_at DESC
LIMIT 1 BY
    network_id,
    if(
        event_type = 'bridge_transfer' AND correlation_key != '',
        concat(
            'bridge|',
            tx_hash,
            '|',
            bridge_direction,
            '|',
            correlation_key
        ),
        concat('event|', event_id)
    );
