# AML-06 validation — Bitcoin semantic evidence

Validated **2026-10-29**, with Rust 1.92 and disposable ClickHouse
**23.8.16.16**. All blocks and attributions are synthetic. No production database,
real Bitcoin node, VM deployment or live Neo4j was used.

## Completed checks

- **56 ordinary Bitcoin tests** pass: 27 library, 11 ingestion/RPC-control,
  16 probe and 2 warehouse-mapping tests. Eight new library cases cover pattern
  thresholds/negative matches, distinct subjects, script/zero output handling,
  coinbase and oversized transactions, exact integers, occurrence identity,
  graph-key/native-row equivalence, version/config changes and pattern-only
  `UNKNOWN` risk. Config/semantic API authentication and invalid options are
  checked before storage access.
- **8 relevant ClickHouse integration tests** pass across the successful runs:
  2 publication/migration pipeline, 4 activity/exposure/risk, 1 intelligence and
  1 new semantic acceptance test. The latter writes native consolidation,
  fan-out, possible CoinJoin and peel-step facts, verifies persisted counts and
  detector manifests, and reads them through the canonical view.
- The semantic acceptance test verifies a real three-step spent-outpoint chain
  and excludes an unrelated same-address peel step. Stored/local events and
  derived chain events are distinguished and deduplicated. A previous-writer
  block with no semantic rows produces historical fallback without changing its
  completed counts. Scope filters select roots while linked context may extend
  beyond them. A 257-leg transaction is skipped with partial coverage; valid
  smaller evidence for the same address remains available. More than 25 roots
  reports partial coverage. Rollback hides orphaned facts and invalidates the
  old persisted assessment. Standalone semantic GETs do not write assessments.
- The native event bodies are captured exactly in persisted risk inputs.
  Pattern-only/CoinJoin-only investigations retain null score / `UNKNOWN`.
  No new scoring rule or weight was introduced.
- **9 central tests** pass, including 2 acceptance tests reading actual adapter
  JSON. The semantic path checks create, persisted payload, Export receipt and
  reopen for peeling and possible CoinJoin. Event/coverage/snapshot/fingerprint
  data survive unchanged. Tampered evidence fails the binding; changed detector
  hash or version becomes stale; missing config becomes unverified. Historical
  evidence is preserved and current risk remains `UNKNOWN`. HTTP sources and
  Neo4j storage are mocked; this is not live Neo4j acceptance.
- **15 shared-policy tests**, **102 Python tests**, dictionary regeneration and
  offline check, **6 JavaScript routing tests**, JS syntax, Rust formatting and
  Git whitespace checks pass. Four network Compose files plus backup overlays
  and the main VM render with dummy credentials. ClickHouse/database/volume
  isolation and policy build contexts are preserved. API options are documented
  in OpenAPI; no separate YAML parser was available in this environment.
- The in-app browser loaded the actual Bitcoin HTML/JS with the generated
  central acceptance payload. Export and saved-investigation selection restored
  5 events: 4 canonical peel-step facts and 1 computed chain. Expanding the chain
  showed all 3 transactions and the exact 8000/6400/5000-satoshi output links.
  `Saved`, explicit scope/storage provenance and `UNKNOWN` risk were verified.
  The pattern section also renders as stacked rows at narrow widths. Browser
  console error list was empty. Browser persistence endpoints were local fixtures.

Initial attempts found and fixed a mismatch between qualified graph edge keys
and native relationship IDs; a regression case now checks identical semantic
witnesses. The first concurrent risk integration attempt also encountered a
closed ClickHouse HTTP connection; the fresh isolated retry passed. No transport
retry or production reliability claim is inferred from that retry.

## Evidence and reproduction

Ignored synthetic artifacts are under `test-results/aml06/`: `bitcoin-unit-tests.log`,
`bitcoin-lib-final.log`, `bitcoin-tests.log` (successful pipeline portion),
`exposure-final.log` (3 passed; transient risk transport failure), `risk-final.log`
(successful retry), `intelligence-final.log`, `semantic-final.log`,
`central-tests.log`, `policy-tests.log`, `python-tests.log`, adapter/central
`*-evidence.json`, `browser-receipt.json` and `semantic-details.png`.

On a **fresh disposable** ClickHouse with the synthetic user `bitcoin_test`,
password `bitcoin-test-only` and fixture database creation rights:

```bash
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
BITCOIN_SEMANTIC_ACCEPTANCE=/tmp/bitcoin-semantics.json \
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml \
  --test semantics -- --ignored --nocapture
```

The test uses `bitcoin_semantic_test` and requires it to be empty. The task-owned
ClickHouse container and local preview server are removed after verification;
existing containers, Docker network, caches and source changes are preserved.

## Remaining limits

Schema **v5 / 39 tables / 53 views / 682 physical table columns** is unchanged,
checksum `8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`.
There is no schema/server consolidation or change to the other chain adapters.

This is bounded, versioned pattern evidence, not a full-chain detector benchmark
or a measured CoinJoin classifier. Confidence is not crime probability. The
tests are amount-preserving fixtures, not consensus-valid mainnet blocks or a
historical BIP30 network acceptance run. Ordinary tests leave 12 other database
ingestion-resilience cases ignored; those were not rerun for this stage.

Historical fact reprocessing (ING-04), clustering, continuous workers, remaining
asset metadata/holdings work, legacy cutover, release images and real
node/VM/Neo4j/capacity acceptance remain open. [Method/API](SEMANTICS.md),
[next stage and full TODO](../TODO_BITCOIN.md).
