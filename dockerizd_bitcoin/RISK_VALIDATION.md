# Bitcoin AML-04 validation — 2026-10-28

The existing evidence policy now writes and reads Bitcoin policy/rule definitions,
assessments, contributions and signal evidence in the **existing local v5 tables**.
The central consumer recomputes the result from the same Rust policy and verifies
the stored assessment's snapshot. Schema remains 39 tables, 53 views and 682
physical columns. No additional runtime service, database or scoring weights.

## Executed

- **46 ordinary Bitcoin Rust tests** pass under Rust 1.92 with locked dependencies.
  The new numeric regression accepts equivalent stored float contributions but
  rejects rounded equality between distinct exact UInt64 revisions, including
  values above 2^53 and near UInt64 maximum. NULL remains distinct from zero.
- **Five ClickHouse integration tests** pass on disposable **23.8.16.16**:
  two pipeline/legacy/restart/UTXO/reorg regressions, AML-01 intelligence,
  AML-03 exposure, and the new AML-04 risk acceptance. The other ignored
  resilience suites were not rerun in this step. This is **51 Bitcoin tests**
  executed successfully, counting each test once.
- The new risk acceptance checks waiting before the first published block,
  persisted UNKNOWN/null with zero signals after initialization, six stored
  rules, direct-label and exposure scoring, and mixed-input context without
  a positive score. It reads the actual structured assessment and signal rows,
  reopens them with a fresh database client, and verifies identical results.
- Concurrent assessments get distinct IDs and identical policy results.
  Fault injection writes a complete-looking parent and all signal rows without
  a publication seal; inspect keeps it historical and does not expose current
  risk. This simulates an interrupted publication state, not a process-kill test.
- Approval of a previously absent seed, source deactivation, revocation, chain
  fence changes and a conflicting stored policy rule invalidate current use.
  Historical risk/evidence remain readable. A conflicting policy version cannot
  be silently overwritten. Wrong-subject reads fail; unauthorized HTTP fails;
  successful snapshot reads use no-store; caller-supplied POST scores are refused.
- The **15 existing policy regression tests** moved unchanged to the shared
  policy crate and pass, including TRON direction, full-width EVM amounts,
  service boundaries, unsupported evidence and correlated paths. The seven
  ordinary central tests and **two central acceptance tests** also pass:
  **nine central tests**, separate from the 15 shared policy tests.
- Central acceptance consumes actual adapter JSON from the ClickHouse fixtures.
  It asserts exact policy, evidence and result hashes across the two binaries,
  then exercises actual create/reopen/Export handlers through local HTTP and
  a Neo4j query-store mock. Invalidating only the assessment suppresses current
  risk; unavailable assessment verification yields unverified. Stored evidence
  and historical score survive. These are not live Neo4j or multi-VM tests.
- Exact parity caught differing float parsing between JSON consumers. The shared
  dependency now enables arbitrary_precision in both programs, preserving the
  same numeric representation through transport. The acceptance above checks
  hashes and computed results, rather than tolerating a mismatch.
- Actual `bitcoin_aml risk` help, compute/persist, inspect and stored-policy CLI
  retrieval pass in separate processes with an unusable RPC URL and no `.env`.
  The CLI uses the required `bitcoin_aml` database; no real credentials or labels.
- The in-app browser loads actual Bitcoin HTML/JS with central acceptance JSON
  served locally. Current view shows the assessment reference, existing
  55/MEDIUM signal and evidence. Historical view keeps the same reference and
  paths while displaying Historical and UNKNOWN. Screenshots were inspected.
- **102 Python tests**, dictionary regeneration/offline validation, all four
  network Compose configurations and main Compose with the policy named build
  contexts pass. Formatting and Git whitespace checks pass.

Private synthetic proof stays ignored under `test-results/aml04/`, including
`bitcoin-tests.log`, `regression-tests.log`, `risk-tests.log`, `policy-tests.log`,
`central-acceptance.log`, `risk-evidence.json`, `exposure-evidence.json`,
`intelligence-evidence.json`, `central-evidence.json`, `cli-receipt.json`,
`current-panel.png` and `historical-panel.png`. The task-owned ClickHouse and
preview server were removed after checks; build caches were retained.

## Reproducing the new acceptance

Use a fresh disposable ClickHouse with fixture user `bitcoin_test`, password
`bitcoin-test-only`, and database creation permission. The test creates the fixed
`bitcoin_risk_test` database and deliberately alters fixture policy rows:

```bash
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
BITCOIN_RISK_ACCEPTANCE=/tmp/bitcoin-risk.json \
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml \
  --test exposure risk_policy_assessments_signals_seals_and_freshness \
  -- --ignored --nocapture

cargo test --locked --manifest-path shared/risk_policy/Cargo.toml
```

## Remaining scope

This completes bounded on-demand risk persistence. Continuous refresh, clustering,
semantic detectors, independent replay, fingerprint/activity completion, legacy
cutover of the other networks, production capacity, release-image builds and
acceptance against real nodes/VMs/live Neo4j remain open. Blocks are synthetic
amount-preserving fixtures, not consensus-valid mainnet blocks. No real source
or claim was imported/approved. A source-file change in the immutable shared
policy requires a new policy version before writing to an existing registry.

[Usage and boundaries](RISK.md), [remaining work](../TODO_BITCOIN.md).
