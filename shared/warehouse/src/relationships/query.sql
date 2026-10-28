-- DB-06: canonical relationship context with exact UTXO witnesses.
-- Rendered only by relationships::read_neighborhood. All {{...}} values are
-- validated identifiers, typed occurrence keys, escaped literals or integer caps.
SELECT toJSONString(tuple({{projection}}, toUInt8(ifNull(w.matched,0))))
FROM {{database}}.aml_address_relationships_canonical AS r
LEFT JOIN
(
    SELECT network_id,block_number,block_hash,block_timestamp_unix_ms,commit_id,revision,tx_hash,transaction_index,
           input_index AS leg_index,'utxo_input' AS leg_kind,prev_tx_hash,toNullable(prev_output_index) AS prev_output_index,
           CAST(NULL,'Nullable(UInt32)') AS output_index,address,
           ifNull(address,concat('script:',lower(hex(SHA256(script_hex))))) AS wallet_subject,
           toUInt256(value_sats) AS amount_raw,toUInt8(1) AS matched
    FROM {{database}}.aml_bitcoin_inputs_canonical
    WHERE network_id={{network}} AND {{witness_selector}}
    UNION ALL
    SELECT network_id,block_number,block_hash,block_timestamp_unix_ms,commit_id,revision,tx_hash,transaction_index,
           output_index AS leg_index,'utxo_output' AS leg_kind,CAST(NULL,'Nullable(String)') AS prev_tx_hash,
           CAST(NULL,'Nullable(UInt32)') AS prev_output_index,toNullable(output_index) AS output_index,address,
           ifNull(address,concat('script:',lower(hex(SHA256(script_hex))))) AS wallet_subject,
           toUInt256(value_sats) AS amount_raw,toUInt8(1) AS matched
    FROM {{database}}.aml_bitcoin_outputs_canonical
    WHERE network_id={{network}} AND {{witness_selector}}
) AS w
ON r.network_id=w.network_id AND r.block_number=w.block_number AND r.block_hash=w.block_hash
   AND r.block_timestamp_unix_ms=w.block_timestamp_unix_ms AND r.commit_id=w.commit_id AND r.revision=w.revision
   AND r.tx_hash=w.tx_hash AND r.transaction_index=w.transaction_index
   AND r.transfer_type=w.leg_kind AND r.event_index=w.leg_index AND r.amount_raw=w.amount_raw
   AND if(r.transfer_type='utxo_input',r.from_subject_id,r.to_subject_id)=w.wallet_subject
   AND ifNull(if(r.transfer_type='utxo_input',r.from_address,r.to_address),'')=ifNull(w.address,'')
   AND ifNull(r.prev_tx_hash,'')=ifNull(w.prev_tx_hash,'')
   AND ifNull(toString(r.prev_output_index),'')=ifNull(toString(w.prev_output_index),'')
   AND ifNull(toString(r.output_index),'')=ifNull(toString(w.output_index),'')
WHERE r.network_id={{network}} AND {{selector}}
ORDER BY r.block_number DESC,r.transaction_index,r.event_index,r.event_sub_index,r.relationship_id
LIMIT {{limit}}
