CREATE TABLE IF NOT EXISTS bitcoin_sync_state (
    id UInt8, revision UInt64, initialized UInt8, start_height UInt64,
    tip_height UInt64, tip_hash String, observed_rpc_height UInt64, published_at UInt64
) ENGINE = ReplacingMergeTree(revision) ORDER BY id;

CREATE VIEW IF NOT EXISTS bitcoin_sync_current AS
SELECT * FROM bitcoin_sync_state ORDER BY revision DESC LIMIT 1;

CREATE TABLE IF NOT EXISTS bitcoin_block_commits (
    block_height UInt64, block_hash String, parent_hash String, block_time UInt64,
    commit_id String, revision UInt64, transaction_count UInt32
) ENGINE = MergeTree ORDER BY (revision, block_height);

-- A commit is visible only after every fact table was synchronously written and
-- the singleton checkpoint was published. Rollback lowers this visibility fence.
CREATE VIEW IF NOT EXISTS bitcoin_blocks_canonical AS
SELECT block_height,
    argMax(block_hash, revision) AS block_hash,
    argMax(parent_hash, revision) AS parent_hash,
    argMax(block_time, revision) AS block_time,
    argMax(commit_id, revision) AS commit_id,
    argMax(transaction_count, revision) AS transaction_count
FROM bitcoin_block_commits
WHERE revision <= (SELECT revision FROM bitcoin_sync_current)
  AND block_height <= (SELECT tip_height FROM bitcoin_sync_current)
  AND (SELECT initialized FROM bitcoin_sync_current) = 1
GROUP BY block_height;

CREATE TABLE IF NOT EXISTS bitcoin_transactions (
    commit_id String, block_height UInt64, block_hash String, block_time UInt64,
    txid String, tx_index UInt32, coinbase UInt8, input_count UInt32,
    output_count UInt32, output_sats UInt64, fee_sats UInt64
) ENGINE = ReplacingMergeTree ORDER BY (commit_id, txid);

CREATE TABLE IF NOT EXISTS bitcoin_outputs (
    commit_id String, block_height UInt64, block_hash String, block_time UInt64,
    txid String, tx_index UInt32, output_index UInt32, coinbase UInt8,
    address String, script_hex String, script_type LowCardinality(String), value_sats UInt64,
    INDEX address_bloom address TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = ReplacingMergeTree ORDER BY (commit_id, txid, output_index);

CREATE TABLE IF NOT EXISTS bitcoin_inputs (
    commit_id String, block_height UInt64, block_hash String, block_time UInt64,
    txid String, tx_index UInt32, input_index UInt32, prev_txid String, prev_vout UInt32,
    address String, script_hex String, script_type LowCardinality(String), value_sats UInt64,
    INDEX address_bloom address TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = ReplacingMergeTree ORDER BY (commit_id, txid, input_index);

CREATE VIEW IF NOT EXISTS bitcoin_transactions_canonical AS
SELECT f.* FROM (SELECT * FROM bitcoin_transactions FINAL) AS f
INNER JOIN bitcoin_blocks_canonical AS b ON f.commit_id = b.commit_id;

CREATE VIEW IF NOT EXISTS bitcoin_outputs_canonical AS
SELECT f.* FROM (SELECT * FROM bitcoin_outputs FINAL) AS f
INNER JOIN bitcoin_blocks_canonical AS b ON f.commit_id = b.commit_id;

CREATE VIEW IF NOT EXISTS bitcoin_inputs_canonical AS
SELECT f.* FROM (SELECT * FROM bitcoin_inputs FINAL) AS f
INNER JOIN bitcoin_blocks_canonical AS b ON f.commit_id = b.commit_id;

-- Choose the latest occurrence of an outpoint (historical BIP30 duplicates).
CREATE VIEW IF NOT EXISTS bitcoin_utxos AS
SELECT o.* FROM (
    SELECT * FROM bitcoin_outputs_canonical
    ORDER BY block_height DESC, tx_index DESC LIMIT 1 BY txid, output_index
) AS o LEFT JOIN (
    SELECT prev_txid, prev_vout, max(tuple(block_height, tx_index)) AS spent_at, toUInt8(1) AS spent
    FROM bitcoin_inputs_canonical GROUP BY prev_txid, prev_vout
) AS i ON o.txid = i.prev_txid AND o.output_index = i.prev_vout
WHERE ifNull(i.spent, 0) = 0 OR i.spent_at < tuple(o.block_height, o.tx_index);
