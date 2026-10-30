# Bitcoin AML-05 validation — 2026-10-28

Scoped on-demand fingerprint, UTC activity, native asset flows, recent transaction
occurrences and directional transaction-peer context now reach the API, panel and
central investigation snapshot. Existing per-VM ClickHouse and shared schema v5
remain unchanged: 39 tables, 53 views, 682 physical table columns. No worker,
migration, classifier or additional risk weight was added.

## Executed checks

- **48 ordinary Bitcoin Rust tests** pass with Rust 1.92 and locked dependencies.
  New tests reject inverted ranges, invalid directions/unknown fields and display
  limits, and preserve negative one-satoshi flow and totals above UInt64 exactly.
- **Seven ClickHouse integration tests** pass on a disposable **23.8.16.16**:
  two new activity acceptances, existing exposure/risk/intelligence acceptances
  and both pipeline/backfill/restart/UTXO/reorg regressions. Counting each once,
  **55 Bitcoin tests** passed. Other ignored resilience suites were not rerun.
- The first new acceptance writes four synthetic blocks through the actual
  extractor/writer on separate UTC days. It checks received/spent legs and values,
  distinct occurrences, both-side activity, fees once per involved transaction,
  exact UTC boundaries, inclusive time/block filters, all direction modes,
  coinbase, transaction shape and script peers. No wallet fee share or allocated
  peer payment is invented. UInt256 SQL aggregation above UInt64 also passes.
- Display caps truncate daily/recent/peer lists independently, while full summary
  totals and unique subject counts remain intact. Empty scopes have null first/
  last activity. Before ingestion the result waits for blocks; explicit future
  block bounds are identified as outside the ingested window. A physically
  inserted unpublished orphan does not affect canonical totals. Invalid HTTP
  options return 400 and successful reads have no-store.
- The second new acceptance retains two occurrences with the same txid in
  different blocks after the earlier output was spent. Both are counted, with
  distinct occurrence IDs. Actual publication rollback hides the abandoned
  branch; appending a replacement does not resurrect it. A window starting at
  height ten reports partial history and preserves a one-satoshi output.
- Filtered investigation persists its fingerprint and filter scope in the risk
  assessment evidence. A fixture has a filtered net flow of **-51 satoshis** while
  its full-window holdings remain **9 satoshis**; this distinction is checked.
- **Seven ordinary central tests and two central acceptances** pass. The activity
  extension consumes actual Bitcoin ClickHouse JSON, checks query forwarding and
  preserves fingerprint, daily rows, peers, asset flows, coverage and query scope
  through create, reopen and reopening after Export. Export itself remains a
  save receipt. Oversized-activity upstream responses ask the caller to narrow
  filters without returning internal query details. Neo4j storage is mocked;
  these are not live Neo4j or multi-VM acceptance tests.
- The in-app browser loads actual HTML/JS with these acceptance responses on a
  local fixture server. The real filter form submits block 2 through 2, direction
  spent. It renders exact received/spent/negative net amounts, UTC day, script
  peer and scoped transaction count while keeping holdings separate. Export and
  reopening restore the saved filters after editing them locally. The receipt
  and screenshot are retained; browser persistence endpoints are local fixtures.
- **15 existing shared-policy tests**, **102 Python tests**, dictionary offline
  validation, **six JavaScript routing tests**, JS syntax and Rust formatting
  checks pass. Main and four network Compose files render with dummy credentials.
  OpenAPI endpoint/options are documented; local references and Git whitespace
  are checked.

Successful logs and synthetic receipts are ignored under `test-results/aml05/`:
`regression-tests.log`, `activity-final.log`, `central-tests.log`,
`activity-evidence.json`, `exposure-evidence.json`, `risk-evidence.json`,
`intelligence-evidence.json`, `central-evidence.json`, `browser-query.json` and
`activity-panel.png`. The task-owned ClickHouse and preview server were removed
after verification. Existing networks, caches, real credentials and databases
were preserved.

## Reproduce the activity tests

Use a fresh disposable ClickHouse with user `bitcoin_test`, password
`bitcoin-test-only` and permission to create fixture databases. The tests use
`bitcoin_activity_test`, `bitcoin_activity_occurrences` and `bitcoin_activity_partial`:

```bash
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
BITCOIN_ACTIVITY_ACCEPTANCE=/tmp/bitcoin-activity.json \
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml \
  --test exposure activity_ -- --ignored --nocapture
```

## Limits and remaining work

Blocks are amount-preserving synthetic fixtures, not consensus-valid mainnet or
a historical BIP30 network validation. Counterparties are transaction context,
not allocated payments or identified owners. Analytics filters do not constrain
holdings, graph or exposure; their scopes stay explicit. Day and transaction
lists are bounded and are not a general paginated history API. SQL scope is
capped at 100000 matching transactions, with per-query time/memory/thread caps;
production performance on a full chain has not been benchmarked.

AML-06 semantic evidence, clustering, continuous workers, remaining holdings/
metadata work, independent replay, legacy cutover, release images and real
node/VM/Neo4j acceptance remain open. [Meaning and API](ACTIVITY.md),
[remaining work](../TODO_BITCOIN.md).
