# AML-07 validation — native BTC metadata and holdings

Validated **2026-10-29**, Rust 1.92 and disposable ClickHouse **23.8.16.16**.
Data is synthetic and amount-preserving, not consensus-valid mainnet blocks.
No production database, real node, destination VM or live Neo4j was used.

## Verified behavior

- **60 ordinary Bitcoin tests** pass: 31 library, 11 ingestion/RPC-control,
  16 probe and 2 mapping tests. New unit cases cover complete native metadata,
  conflict rejection, null supply/observation fields, honest system verification,
  coinbase boundaries/overflow, strict limits/options and not assigning genesis
  P2PK or OP_RETURN scripts to an address.
- **9 relevant ClickHouse integration tests** pass across the successful runs:
  2 pipeline/migration, 4 activity/exposure/risk, 1 intelligence and 2 new holdings
  cases. No `bitcoin_*` schema is added on a fresh installation; existing legacy
  migration/publication/reorg behavior remains covered.
- The main holdings fixture publishes **101 consecutive synthetic blocks**, then
  checks coinbase behavior at tips 98, 99 and 100: mature-at-tip and next-block
  eligibility differ at 100 confirmations. Exact balance is **299 satoshis**
  (`0.00000299` BTC), with 2 unspent and 1 spent latest outpoint. Spending proof
  points to the actual transaction/input; a one-row list limit leaves totals
  unchanged. Empty history returns null; complete ingested history can return a
  known zero. `spendable_sats` stays null.
- The native metadata initializer is idempotent and persists one definition.
  A deliberately conflicting symbol fails metadata reads, readiness and API
  access; reinitialization does not append a repair/approval. Registry row counts
  are checked. Protocol metadata is independent of the chain fence and is not an
  imported attribution or circulating supply estimate.
- The occurrence fixture checks repeated txids with changed values/addresses:
  an old spend does not consume the new occurrence; an old address does not retain
  an overwritten output. Observed zero in partial history stays `partial` /
  `is_complete_balance=false`. Rollback restores prior spent/unspent states.
  The main case also replaces the last branch: holdings become 100 satoshis,
  1 unspent and 2 spent outpoints, with a different source snapshot.
- The final query optimization identifies candidate wallet outpoint keys, then
  retains **all occurrences and spends of those keys** before address filtering.
  The occurrence integration was rerun after this optimization, and direct SQL
  verified the replacement branch's 100/1/2 amount/count results. There is no
  claim of full-chain performance benchmarking.
- **9 central tests** pass with the newly generated activity/exposure/intelligence
  and holdings JSON. The earlier semantic fixture is also consumed to exercise
  backward compatibility. Create, persisted payload, Export receipt and reopen
  preserve the entire new holdings object and query scope. Tampered amounts fail
  binding; changed metadata becomes stale; missing metadata becomes unverified;
  changed source invalidates the saved result. Historical holdings are preserved
  and current risk becomes `UNKNOWN` when freshness cannot be established.
- The central consumer previously changed `holdings.persisted` before storage.
  It now preserves framed Bitcoin holdings unchanged, so the source evidence hash
  remains valid. The central investigation metadata describes central storage;
  the chain response accurately describes its computed quantities and persisted
  source facts. Other adapters and older unframed snapshots keep their existing
  path. Neo4j storage and HTTP upstreams in these tests are mocked.
- **15 shared-policy tests**, **102 Python tests**, **6 JavaScript routing tests**,
  JS syntax, Rust formatting, dictionary regeneration/offline check, local doc
  links and Git whitespace checks pass. Main/four network Compose definitions
  and backup overlays render with dummy credentials; independent database/volume
  topology is preserved. OpenAPI endpoints/options are documented; no separate
  YAML parser was available here.
- In-app browser verification uses the actual HTML/JS and captured test payloads.
  Export and saved-investigation selection restore the exact 299-satoshi balance,
  maturity, verified source and all three output states, including the 50-satoshi
  spent output. The empty adapter fixture renders **— / balance is unknown**,
  rather than zero. Console error list is empty. Browser persistence endpoints
  are local fixtures, not a live Neo4j acceptance run.

## Evidence and reproduction

Ignored artifacts are under `test-results/aml07/`: `bitcoin-tests.log`,
`holdings-final.log` (main case), `holdings-occurrences-final.log`,
`related-db-tests.log`, `intelligence-tests.log`, `central-final-tests.log`,
`policy-tests.log`, `python-tests.log`, adapter/central `*-evidence.json`,
`browser-receipt.json`, `browser-waiting.json`, `holdings-panel.png` and
`holdings-waiting.png`. The task-owned ClickHouse/preview server are removed
after checks; Docker network, caches and pre-existing source changes are retained.

On a fresh disposable ClickHouse with fixture database creation rights:

```bash
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
BITCOIN_HOLDINGS_ACCEPTANCE=/tmp/bitcoin-holdings.json \
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml \
  --test holdings -- --ignored --nocapture
```

The fixture user/password are `bitcoin_test` / `bitcoin-test-only`; the databases
are `bitcoin_holdings_test` and `bitcoin_holdings_occurrences`. Use empty
databases. The main test deliberately leaves conflicting metadata in its
disposable database to verify rejection. An initial test helper assumed every
400 response was JSON; it was corrected to accept Axum's query-extraction text
responses before rerunning the main case.

## Limits and remaining work

Schema remains **v5 / 39 tables / 53 views / 682 physical columns**, checksum
`8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`.
No new DDL, holding/run table, RPC, crate, shared database/server, risk weight or
automatic intelligence approval is introduced. Existing installations need the
updated schema job to bootstrap their missing native metadata row.

Decoded address scripts are the ownership scope; script-only outputs and private
wallet semantics are not inferred. Output lists are bounded, not a full paginated
history API. Production capacity/release images, real node/VM/Neo4j acceptance,
legacy cutover, clustering, continuous workers and independent historical replay
remain open. The 12 other database ingestion-resilience cases and standalone
semantic integration were not rerun in this stage.

[Method/API and initialization](HOLDINGS.md) · [Remaining TODO](../TODO_BITCOIN.md)
