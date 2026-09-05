-- Keep ingestion failures block-scoped. Transaction failures are represented by
-- the owning block and a sanitized summary, so an always-empty tx_hash wastes space.

ALTER TABLE {{database}}.ingestion_failures
    DROP COLUMN IF EXISTS tx_hash;

ALTER TABLE {{database}}.ingestion_benchmarks
    ADD COLUMN IF NOT EXISTS blocks_per_second Float64 AFTER elapsed_ms;

ALTER TABLE {{database}}.ingestion_benchmarks
    ADD COLUMN IF NOT EXISTS observed_live_blocks_per_second Float64 AFTER blocks_per_second;

ALTER TABLE {{database}}.ingestion_benchmarks
    ADD COLUMN IF NOT EXISTS live_rate_multiple Float64 AFTER observed_live_blocks_per_second;
