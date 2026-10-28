# DB-05 validation — 2026-10-27

This is the DB-05 stage report. Relationship interpretation is now completed
within the scope of [DB-06](DB06_VALIDATION.md); the next stage is DB-07.

The analytics/specialized target contract is version **4**, with **38 tables,
40 views and 668 physical table columns**. Tests used disposable ClickHouse
23.8.16.16 and Rust 1.92. No deployed VM or production database was inspected,
backfilled or cut over. Network databases remain independent.

## Results and scope

| Check | Result |
|---|---|
| Shared Rust | **29 passed**: 7 unit, 7 value, 2 core, 1 analytics contract; 12 real ClickHouse integration tests |
| Bitcoin Rust | **53 passed**: 10 unit, 2 pipeline, 23 resilience, 16 probe, 2 mapping; includes 14 ClickHouse tests |
| Native drivers | **2 passed**, exact checked-in TRON/BSC integration test sources using 0.14.1/native-tls and 0.14.3/default features |
| Python | **66 passed**, including frozen-baseline/current-target separation, mapping provenance and stale view rejection |
| Clippy | Shared crate and Bitcoin `--all-targets -- -D warnings`, passed |
| Formatting | Both crates `cargo fmt --check`, passed |
| Exports | Frozen v1/v2/v3 SQL, v4 SQL, frozen v3 JSON and current effective JSON matched their Rust exporters |
| Real schema CLI | `bitcoin_init_schema` and `--check` returned version 4, 38 tables, 40 views and the expected checksum |
| Build/source parity | 46 Rust source/test/example files matched the staged build after line-ending normalization |
| Dictionary | Regenerated source references; offline `--check` passed; DB-01 materialized baseline snapshot unchanged |

The full Bitcoin suite passed. Subsequent changes affected the new analytical
views and the extended EVM-log projection. After those corrections, all new
DB-05 integration cases were rerun on fresh fixtures; the existing eight shared
ClickHouse regression cases also passed. Final non-ClickHouse shared tests,
Bitcoin unit tests, both native drivers, binary build, actual fresh schema
initialization/check, export checks and strict Clippy passed on the final source.
The native tests ran in temporary minimal Cargo harnesses, avoiding unrelated
application binaries; no production dependency versions or manifests changed.

## Four-network acceptance

| Fixture database | Tables | Views | Physical table columns |
|---|---:|---:|---:|
| db05_review_tron | 38 | 40 | 668 |
| db05_review_ethereum | 38 | 40 | 668 |
| db05_review_bsc | 38 | 40 | 668 |
| db05_review_bitcoin | 38 | 40 | 668 |

Complete normalized CREATE definitions and physical column metadata have
identical SHA256 digests across these four databases. Each retains migrations
1, 2, 3 and 4 with its own network identity. Counts, checksums, identities and
normalized schema digests are recorded in [db05-acceptance.json](db05-acceptance.json).

V3-to-v4 compared all original-column projections of all applicable extended
tables byte-for-byte as JSON. New verification/supply/operation fields remained
unknown where the old schema had no evidence. The test additionally exercised
all 25 supported analytics/specialized reader families, including empty views.
V1/v2 upgrade regression tests also passed; no historical records were rewritten.

Observed behavior covered:

- Column, new-table and view installation resumes after interruption. Unknown
  partial table shapes and removed applied constraints are rejected.
- Approved entity/exchange/claim/protocol projections respect later revocation,
  claim expiry and source deactivation. An earlier approval cannot reappear.
- Invalid confidence and an approval without a reviewer are rejected by ClickHouse.
- Raw envelopes, discovery, Bitcoin scripts, EVM traces **and EVM logs** use the
  exact network/height/hash/commit/revision block fence. UInt256 values, ordered
  trace positions, nullable receipts and opaque original JSON round-trip exactly.
- Specialized data for an inapplicable network is rejected. Bitcoin receipt
  capability stays NULL, and native BTC supply cannot exceed the satoshi bound.
- Queries remain network-scoped; native SDK validation still works with readonly=1.

## Defects caught during validation

The integration tests exposed nested join views returning alias-prefixed output
names. Projections now give each column its explicit contract name. A temporary
manifest generator also duplicated projected extensions when it read a mutable
current export; it was corrected to use the frozen v3 definition.

A source-deactivation test showed that FINAL alone could allow a state predicate
to reach the source scan before replacement, returning an older active record.
Approved projections now select a complete latest row with argMax(tuple(...),
revision) before filtering state. The full four-network review test passed after
this correction. Coverage of every supported reader also ensured that the new
EVM-log columns are exposed through its canonical projection.

Only disposable task fixtures contained intermediate development schemas. Their
failed/obsolete versions were discarded with their task-owned containers; no
applied production migration or historical v1/v2/v3 definition was edited.

## Immutable identities and remaining work

- v1: `147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`
- v2: `c76d93f7c782fc09b7a87d5b7a0337203b595ca60fcd3fc6ca85092c2bb9b5f5`
- v3: `9548e226c0d1d4259fe488bab1d9a137232e6e68d311054190b0034db7e0800e`
- v4: `955e0c5ff2e6bb7000793e66408312f4d0172d026f15f328a9431ee050b568ec`

This completes the **target analytics/specialized schema, migration and common
reader** scope of DB-05. Legacy writer/reader/history cutover and backup restore
remain DB-08; relationship semantics and invalidation remain DB-06/DB-07.
Bitcoin AML workers and APIs remain AML-01 through AML-08. Existing TRON ML and
main-VM Analytical Node data remain in their current systems. No new Bitcoin ML,
ING-04 replay or production rollout was performed.

Only fixture credentials were used for database access. Task-owned containers
were removed after verification; the existing Docker network and build caches
were retained. Local environment files and Git history were unchanged.
