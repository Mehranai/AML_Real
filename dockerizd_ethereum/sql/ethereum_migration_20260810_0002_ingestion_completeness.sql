-- Record what evidence was actually available while ingesting each block.

ALTER TABLE {{database}}.ingested_blocks
    ADD COLUMN IF NOT EXISTS receipt_data_complete UInt8 DEFAULT 0 AFTER ingestion_status;

ALTER TABLE {{database}}.ingested_blocks
    ADD COLUMN IF NOT EXISTS trace_data_complete UInt8 DEFAULT 0 AFTER receipt_data_complete;

ALTER TABLE {{database}}.ingested_blocks
    ADD COLUMN IF NOT EXISTS rpc_provider LowCardinality(String) DEFAULT '' AFTER trace_data_complete;
