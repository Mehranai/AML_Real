# Bitcoin AML-01 validation — 2026-10-27

Completed the practical source → pending import → separate review → address
investigation → existing central label-risk policy → panel path. Schema stays
v5 (39 tables, 53 views, 682 physical columns). No new migration, ML model or risk
weight was introduced. Each network VM keeps its independent ClickHouse.

## Exercised

- **43 ordinary Bitcoin Rust tests** pass, including four new intelligence input,
  review/version, validity and approval-anchor tests. Docker uses Rust 1.92 and
  existing locked dependencies; Rust sources were formatted.
- **One ClickHouse intelligence integration** on a fresh disposable 23.8.16.16
  server passes pending/approved/rejected/revoked, dry-run, source inactivity,
  claim edits resetting approval, expected-revision conflict, expired/future
  claims, endpoint authorization/no-store and interrupted review publication.
- Forced FINAL compaction of claims/reviews; approved subject and source evidence
  survives in unique review records even after old physical claim rows disappear.
- Both existing **ClickHouse pipeline regressions** pass: legacy repair/backfill,
  restart, UTXOs, paths, visibility and reorg. No production database was used.
- **22 ordinary central tests** pass. A separate central acceptance test consumes
  the actual synthetic adapter JSON from the ClickHouse test: pending, revoked
  and inactive-source labels remain UNKNOWN; the approved explicit fixture risk
  designation produces the existing APPROVED_RISK_LABEL signal (70 points).
  Missing intelligence provenance is refused.
- The same central test serves source status/intelligence snapshots over local
  HTTP. With chain state unchanged, source deactivation/revocation makes the old
  approved investigation stale, suppresses current risk and preserves its old
  assessment. Missing intelligence verification yields unverified. Existing
  reopen/Export regressions also pass. These HTTP/store fixtures are not a live
  Neo4j/VM deployment.
- Actual `bitcoin_aml intelligence` CLI help, stdin source, dry-run, pending
  import, review, claims, revoke and durable history pass without any RPC config.
  CLI retains the required `bitcoin_aml` database isolation.
- **102 Python tests** and dictionary regeneration/offline check pass. The
  dictionary now records the active intelligence reader/writer ownership.
- In-app browser inspected the actual panel using synthetic ClickHouse/central
  attribution/risk JSON with the existing graph/holdings UI fixture: approved
  source/reviewer, evidence and 70/HIGH signal display; the historical variant
  shows `approved / historical`, a warning and UNKNOWN current risk. Screenshots
  were inspected and saved under ignored `test-results/aml01/`. Host Edge returned
  no headless DOM, so it is not counted as a passing headless test.

Private synthetic receipts/logs are in `test-results/aml01/`: `bitcoin-tests.log`,
`central-tests.log`, `central-proof.log`, `clickhouse-evidence.json`,
`central-evidence.json`, `approved-panel.png` and `historical-panel.png`.
No labels or sources are imported automatically at startup. Example files carry
no real attribution, and their review revision placeholder cannot be approved.

## Still open

Directional exposure, clustering, semantic detectors, ongoing analytics workers,
shared risk-assessment persistence, four-network legacy cutover, production
capacity and real VM/Neo4j acceptance are not completed by AML-01. The operator
CLI records declared reviewer identity; organizational RBAC is not added here.
The durable audit is review decisions with captured evidence, not every pending
edit or every source-registry version. A written review alone is not proof that
its matching claim publication completed; the active reader checks both.

Deploy the central freshness check before the updated Bitcoin adapter. Real
intelligence files belong in the ignored `intelligence-data/` directory.
[Usage and limits](INTELLIGENCE.md), [remaining work](../TODO_BITCOIN.md).
