-- Latest canonical occurrence per txid/output index is selected BEFORE filtering
-- address (BIP30 can replace an outpoint with a different address/value).
-- Candidate keys may be found via any historical address occurrence, but all
-- occurrences of those keys must participate in the latest/spend selection.
WITH keys AS (
    SELECT DISTINCT tx_hash,output_index FROM aml_bitcoin_outputs_canonical
    WHERE network_id={network:String} AND address={address:String}
), latest AS (
    SELECT * FROM aml_bitcoin_outputs_canonical
    WHERE network_id={network:String} AND tuple(tx_hash,output_index) IN (SELECT tx_hash,output_index FROM keys)
    ORDER BY block_number DESC,transaction_index DESC
    LIMIT 1 BY tx_hash,output_index
), spends AS (
    SELECT prev_tx_hash,prev_output_index,
        max(tuple(block_number,transaction_index)) AS spent_at,
        argMax(tuple(block_number,block_hash,tx_hash,input_index,commit_id,revision),tuple(block_number,transaction_index)) AS spending
    FROM aml_bitcoin_inputs_canonical WHERE network_id={network:String}
        AND tuple(prev_tx_hash,prev_output_index) IN (SELECT tx_hash,output_index FROM keys)
    GROUP BY prev_tx_hash,prev_output_index
), states AS (
    SELECT o.*,
        toUInt8(ifNull(i.spent_at,tuple(toUInt64(0),toUInt32(0)))>tuple(o.block_number,o.transaction_index)) AS is_spent,
        i.spending AS spending
    FROM latest AS o LEFT JOIN spends AS i ON o.tx_hash=i.prev_tx_hash AND o.output_index=i.prev_output_index
    WHERE o.address={address:String}
)
