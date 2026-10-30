-- Active AML-05 query prefix. Occurrences include all canonical publication keys.
-- Only typed parameters and the validated direction expression are substituted.
WITH wallet_legs AS (
    SELECT network_id,block_number,block_hash,block_timestamp_unix_ms,commit_id,revision,tx_hash,transaction_index,
           countIf(side='input') AS spent_input_count,countIf(side='output') AS received_output_count,
           sumIf(toUInt256(value_sats),side='input') AS spent_sats,
           sumIf(toUInt256(value_sats),side='output') AS received_sats
    FROM (
        SELECT network_id,block_number,block_hash,block_timestamp_unix_ms,commit_id,revision,tx_hash,transaction_index,value_sats,'input' AS side
        FROM aml_bitcoin_inputs_canonical
        WHERE network_id={network:String} AND address={address:String}
          AND block_number BETWEEN {from_block:UInt64} AND {to_block:UInt64}
          AND block_timestamp_unix_ms BETWEEN {from_time:UInt64} AND {to_time:UInt64}
        UNION ALL
        SELECT network_id,block_number,block_hash,block_timestamp_unix_ms,commit_id,revision,tx_hash,transaction_index,value_sats,'output' AS side
        FROM aml_bitcoin_outputs_canonical
        WHERE network_id={network:String} AND address={address:String}
          AND block_number BETWEEN {from_block:UInt64} AND {to_block:UInt64}
          AND block_timestamp_unix_ms BETWEEN {from_time:UInt64} AND {to_time:UInt64}
    ) GROUP BY network_id,block_number,block_hash,block_timestamp_unix_ms,commit_id,revision,tx_hash,transaction_index
), involved AS (
    SELECT w.*,t.coinbase,t.input_count,t.output_count,toUInt256(t.fee_raw) AS fee_sats
    FROM wallet_legs AS w INNER JOIN aml_transactions_canonical AS t
      ON w.network_id=t.network_id AND w.block_number=t.block_number AND w.block_hash=t.block_hash
     AND w.block_timestamp_unix_ms=t.block_timestamp_unix_ms AND w.commit_id=t.commit_id
     AND w.revision=t.revision AND w.tx_hash=t.tx_hash AND w.transaction_index=t.transaction_index
), scoped AS (
    SELECT * FROM involved WHERE {{direction}}
)
