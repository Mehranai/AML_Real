# DB-07 validation — 2026-10-27

The final target is schema **5**, **39 tables, 53 views and 682 physical table
columns**, checksum
`8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`.
Versions 1–4 and their generated SQL remain unchanged. V4 is frozen separately
in `analytics-contract-v4.json`; the current export is v5.

## Acceptance exercised

- Upgraded four independent v4 fixtures on ClickHouse **23.8.16.16**, preserving
  every pre-existing asset-metadata field. New provenance defaults remain NULL.
  Repeated initialization succeeds without another migration.
- Interrupted v5 after partial table/column/view installation, resumed it and
  preserved old values. Deliberately drifted an applied view; both validation
  and reinitialization rejected it rather than repairing it silently.
- Verified exact source snapshots and confirmation boundaries, including genesis,
  insufficient depth, zero confirmations and UInt64 MAX. Confirmation depth is
  never reported as consensus finality.
- Checked nine derived tables through all three publication groups:
  cluster runs/claims/memberships/versions, exposure runs/paths/aggregates,
  risk assessments/signals. Unsealed/crashed attempts remain audit-only.
- Checked missing transaction coverage before complete markers, unpublished
  complete blocks, snapshot changes while reading, rollback orphan markers,
  forward re-publication, gap/parent/revision rejection and complete reset.
  Historical facts remain stored; old branches and old analyses do not reappear.
- Checked explicit seal invalidation and later stale-parent replacement, including
  suppression of dependent children. Analysis validity conservatively requires
  the exact current source publication; normal append invalidates old seals too.
- Checked chain metadata against observed hash and source revision, latest-wrong-
  hash suppression, and canonical discovery-backed jobs. Reorg hides both metadata
  and jobs; the unanchored older row cannot reappear.

The four final schema receipts and normalized DDL/column digests are recorded in
[db07-acceptance.json](db07-acceptance.json). Their table/view/column shapes match.
Adversarial and interrupted fixtures are task-owned disposable data, not a
deployed migration history. Intermediate development v5 fixtures were discarded
with their containers; no production-applied schema was rewritten.

## Tests and practical limits

- Shared crate: **23 non-ClickHouse tests**, **two DB-07 ClickHouse tests**,
  **two publication/schema regressions** and **two relationship regressions**.
  DB-07's integration test covers all four networks plus a separate interrupted
  migration fixture. Relationship checks retain multi-leg Bitcoin evidence,
  exact account amounts, occurrence separation and missing/mismatched witnesses.
- Bitcoin: the **53-test suite** passed during implementation. After the final
  metadata/aggregate extensions and API changes, reran the 10 library tests,
  two ClickHouse pipeline tests and the protected failure-API test on fresh
  fixtures. The latter checks `Cache-Control: no-store`; the pipeline checks
  source descriptors and snapshot-ID change after reorg.
- Analytical Node: **22 tests** passed, including HTTP source-status checks,
  stale/unavailable-source handling, and reopen/Export against a mock Neo4j
  HTTP endpoint. These test routing and payload behavior; no real Neo4j server,
  deployed snapshot rewrite or changed storage Cypher is claimed.
- Both native SDK smoke tests passed with the checked-in TRON/BSC test sources
  and existing locked dependency harnesses. New views and validation run with
  `readonly=1`. Full legacy adapter builds/workers were not exercised.
- **69 Python tests**, dictionary regeneration and offline consistency check.
- Strict Clippy passed for the shared crate, Bitcoin and Analytical Node;
  Rust sources were formatted. Generated migration/export checks matched.
- Chromium **156.0.8078.47** passed legacy, typed/script and stale-snapshot UI
  cases. Screenshots were inspected. These are local fixtures, not a live
  four-VM/Neo4j acceptance run.

One relationship regression run hit a transport-level closed connection to the
disposable ClickHouse, with no failed data assertion. The same final-code suite
passed on a fresh isolated fixture. The container was not OOM-killed.

Builds used the cached Rust 1.92 Docker environment. Only disposable fixture
credentials were used; no local environment-file contents were read or printed.
Docker access was used directly under the user's existing authorization.

## Remaining boundaries

This completes the common DB-07 storage/read/publication contract and active
Bitcoin integration. Legacy TRON/Ethereum/BSC writer/reader/history cutover and
backup restore remain DB-08. Their active legacy workers/caches are not claimed
to have adopted the new namespace. No Bitcoin intelligence, clustering, exposure
or risk worker was created by these schema tests. Such workers must use the
snapshot/seal boundary and explicitly invalidate dependencies on policy/review
changes; automatic intelligence lifecycle wiring remains in the AML steps.

Snapshot checks are before/after fences under exclusive cursor ownership, not
distributed snapshots or a multi-writer lease. Saved-source validity is relative
to the upstream's published state at check time; node freshness and consensus
finality are not inferred. All intermediate/final test containers were cleaned
up; the existing Docker network and build caches remain. Git history and VM
configuration were unchanged.
