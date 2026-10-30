# Native BTC metadata and holdings — AML-07

Bitcoin uses the existing shared **`aml_asset_metadata`** table, with the same
schema as the other network VMs. Each VM retains its independent ClickHouse
database and volume. This stage adds no DDL migration or holdings table.

## Native protocol metadata

After shared schema initialization, `src/assets.rs` bootstraps one `native:BTC`
definition: Bitcoin / BTC / 8 decimals / native standard. The definition is
versioned `bitcoin_native_asset_v1`, with a deterministic hash and stable
metadata snapshot. It is verified against the compiled protocol definition,
with pinned [Bitcoin Core amount constants](https://github.com/bitcoin/bitcoin/blob/v28.0/src/consensus/amount.h)
and [coinbase maturity constant](https://github.com/bitcoin/bitcoin/blob/v28.0/src/consensus/consensus.h)
as source references.

`is_verified=1` and the registry's system approval describe **protocol constants**.
The reviewer is explicitly `system:compiled_bitcoin_protocol_definition`;
`verification.human_reviewed=false` and `intelligence_approval=false`. This does
not approve any imported attribution, create an entity, or create a risk seed.
The native definition is independent of block/source revision; its observation
fields remain null. It survives ordinary checkpoint changes and reorgs.

`total_supply_raw` stays **null**. `max_money_sats` is the consensus monetary
range bound, not current circulating supply. `code_hash` is null because BTC is
not an EVM token contract. Unsupported ERC standards, EVM contract metadata/logs
and token discovery are explicitly `not_applicable` in the response capability
map. No placeholder token, discovery job or metadata worker is fabricated.

Initialization is idempotent. A conflicting current metadata row, including one
hidden by a source fence, is rejected rather than overwritten or given a new
approval. Reads validate the visible stored definition; invalid/missing metadata
fails readiness and queries. On an existing installation with no native row,
rerun the existing **schema initialization job** with the updated binary:

```bash
# From the Bitcoin adapter directory, after rebuilding the configured image:
docker compose run --rm bitcoin-schema
# Or, with the configured writer lock and database environment:
bitcoin_aml schema
```

`schema --check` is read-only and does not bootstrap metadata. A conflicting
definition requires deliberate inspection/version-upgrade planning; rerunning
initialization does not silently repair it. No production schema job was run as
part of this task.

## Holdings contract

`src/holdings.rs` reads `sql/queries/holdings.sql` against canonical facts only.
The latest occurrence of `(tx_hash, output_index)` is selected **before address
filtering**. This preserves BIP30 semantics when a later occurrence changes the
address/value: an earlier payment cannot reappear as current holdings. Spending
must occur after that selected output's `(block, transaction_index)`.
Historical address occurrences first identify a candidate key set; all output
occurrences and spends for those keys are retained before the latest/address
decision. This avoids grouping unrelated network outpoints without hiding a
later occurrence that changes the address.

- Totals are exact UInt128 SQL sums, range-checked and returned as satoshi
  strings; BTC decimals are formatted without floating point. Spent outputs do
  not contribute to the balance. `spent_output_count` counts spent **latest
  outpoints**, not all historical spends. Older superseded occurrences remain
  in activity history.
- Before the first committed block, status is `waiting_for_blocks`; amounts and
  counts are **null**, not zero. With a partial ingestion window, status is
  `partial`, `is_complete_balance=false` and amounts mean **observed holdings in
  that window**. Zero observed does not establish a complete zero wallet balance.
  Full ingested history from genesis uses `available` / `is_complete_balance=true`.
- The scope is decoded address scripts, not wallet ownership. Bare public keys,
  multisig participants, script-only and OP_RETURN outputs are not assigned to
  an arbitrary address. This also avoids assigning the genesis coinbase P2PK
  output to a fabricated decoded address. Bitcoin Core separately excludes the
  [genesis coinbase from its UTXO set](https://github.com/bitcoin/bitcoin/blob/v28.0/src/validation.cpp).
- Mempool spends/receipts, private keys and script conditions are not checked.
  `spendable_sats` is **null**. No wallet-specific RPC (`getbalance`, `listunspent`
  or wallet import) is used. Activity net flow remains distinct from holdings.
- The default list contains 25 recent latest-outpoint states, both spent and
  unspent, with origin, exact amount, spending transaction/input when present,
  confirmations and coinbase maturity. The cap is 100. Truncating this list
  never truncates aggregate totals. This is not a complete paginated history API.

For a coinbase created at height H and a snapshot at T:

| Field | Meaning |
|---|---|
| `immature_sats` | Unspent coinbase value where T < H + 100 |
| `mature_sats` | Observed balance minus that immature amount |
| `eligible_next_block_sats` | Unspent non-coinbase plus coinbase where T + 1 >= H + 100 |
| `confirmations` | T − H + 1 |
| `blocks_to_maturity` | max(0, H + 100 − T) |

At 100 confirmations a coinbase is eligible for the **next** block, but is not
yet counted as mature at the current tip. These conventions are explicit and
tested independently; the mature-at-tip convention also matches the
[wallet maturity calculation](https://github.com/bitcoin/bitcoin/blob/v28.0/src/wallet/wallet.cpp). They do not attest script spendability. The consensus
spending-height check is in [Bitcoin Core tx_verify.cpp](https://github.com/bitcoin/bitcoin/blob/v28.0/src/consensus/tx_verify.cpp).

## API and saved investigations

```text
GET /api/bitcoin/assets/native
GET /api/bitcoin/wallet/{address}/holdings?limit=25
GET /api/bitcoin/wallet/{address}/investigation
```

Endpoints use the existing service key, request slots, timeout and `no-store`
protections. Unknown options and invalid limits/addresses return 400. Storage,
metadata, source or query-budget failures return unavailable/error, never zero.
Each holdings SQL query is bounded to 15 seconds, 2 threads and 256 MiB.

`holdings_snapshot` binds the entire returned holdings object, wallet, canonical
source snapshot and native metadata definition. Quantities are computed at read
time: `persisted=false`, `source_facts_persisted` and `metadata_persisted` distinguish
the storage layers. Investigation preserves the full response in the existing
central Neo4j snapshot; no extra ClickHouse holdings/run table is invented.

The central consumer validates holdings hashes and its wallet/source binding,
and compares native metadata with the snapshot returned by `/status`. Changed
source/metadata becomes historical; missing or tampered evidence becomes
unverified. The original holdings stay available as history. Older snapshots
without the new holdings frame retain their earlier compatibility path.

The Bitcoin panel shows verified metadata/source, mature and immature amounts,
spent/unspent output states and coverage. Export and reopening restore the same
holdings data. The shared risk policy and its weights are unchanged.

[Validation](HOLDINGS_VALIDATION.md) · [Remaining work](../TODO_BITCOIN.md)
