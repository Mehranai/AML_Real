# DB-04 validation — 2026-10-26

The v3 target core contract passed validation on disposable ClickHouse
**23.8.16.16** (`clickhouse/clickhouse-server:23.8`), compiled with Rust 1.92.
Each network retains its own database/server/volume deployment. No deployed VM,
production history, or legacy adapter cutover was included in this run.

## Results

| Check | Result |
|---|---|
| Shared Rust crate | **24 passed**: 7 unit, 7 column-value, 2 core-value/pre-I/O, 8 real ClickHouse integration |
| Bitcoin Rust | **53 passed**: 10 unit, 2 pipeline, 23 resilience, 16 probe, 2 mapping; includes 14 ClickHouse tests |
| Native SDK | **2 passed**, exact checked-in TRON/BSC test sources with ClickHouse clients 0.14.1/native-tls and 0.14.3/default features |
| Python | **63 passed**, including dictionary target/baseline separation and stale-export rejection |
| Clippy | Shared crate and Bitcoin, `--all-targets -- -D warnings`, passed |
| Formatting | Shared crate and Bitcoin `cargo fmt --check`, passed |
| SQL exports | Frozen v1, frozen v2, new v3 and effective contract JSON `export_sql --check`, passed |
| Dedicated schema binary | `bitcoin_init_schema --check`: version 3, 29 tables, 21 views, expected checksum, `newly_applied=false` |
| Dictionary | Regenerated from current source references; offline `--check` passed; DB-01 baseline snapshot unchanged |
| Build/source parity | 43 Rust source/example/test files matched the staged build after line-ending normalization |

The complete suites passed. Final edge-case changes were followed by focused
reruns: identity-revision exhaustion (including v1 at `u64::MAX-1` with two
remaining upgrades), the four-network core integration, all non-ClickHouse
shared tests, both Bitcoin mapping tests, interrupted-repair clock rollback and
fallback resolution. Strict Clippy then passed on the final sources. The native
driver checks used temporary minimal Cargo harnesses to avoid building unrelated
application binaries; production manifests and dependency versions were unchanged.

## Database equality and core behavior

| Fixture database | Tables | Views | Physical table columns |
|---|---:|---:|---:|
| db04_core_tron | 29 | 21 | 404 |
| db04_core_ethereum | 29 | 21 | 404 |
| db04_core_bsc | 29 | 21 | 404 |
| db04_core_bitcoin | 29 | 21 | 404 |

Normalized complete CREATE definitions (including views/constraints/keys/indexes)
and physical column metadata have identical SHA256 digests across these four
databases. All retain migration versions 1, 2 and 3, with a v3 network identity.
The machine-readable counts, identities and digests are saved in
[db04-acceptance.json](db04-acceptance.json).

The same core API read all eight families on every network and returned no
foreign-network rows. Partial/unpublished facts and mismatched commit/revision
rows remained invisible. A count mismatch prevented the complete marker. Exact
UInt256 NFT IDs/amounts, ordered trace paths, sub-event positions and JSON evidence
containing quotes/question marks round-tripped unchanged. Benchmark-current
returned the latest revision only. Missing fact-family coverage was not inferred
as zero.

V1-to-v3 preserved the original-column projections of every applicable table
except schema bookkeeping; v2-to-v3 separately preserved every extended core
table's original columns. Old unknown failure timestamps/retryability remained
NULL and old benchmark status remained `unknown`. No historical rows were rewritten.

Interrupted column, constraint and view installation resumed. Wrong types,
defaults, unknown columns, altered views, missing/modified applied constraints,
invalid historical values, missing applied tables and exhausted identity revisions
were rejected without blessing drift. Read-only native-driver checks still passed.

Bitcoin's final focused repair test additionally verified persisted commit row
counts and source block metadata, first/last/resolved failure times under clock
rollback, preserved first-failure time, retryability and interruption evidence.

## Immutable identities and limits

- v1 contract checksum: `147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`
- v2 contract checksum: `c76d93f7c782fc09b7a87d5b7a0337203b595ca60fcd3fc6ca85092c2bb9b5f5`
- v3 contract checksum: `9548e226c0d1d4259fe488bab1d9a137232e6e68d311054190b0034db7e0800e`
- Raw v1 JSON SHA256 remains `0ae654956ac2673cd67be2223b97e545612875007ffc1c57b2dec838b8d3c483`.

Only fixture credentials were used for database access. Task-owned build/test
containers were removed after verification; the existing Docker network and
compiler caches were retained. The user's local environment files were unchanged.

This closes the **target core schema and common access** scope of DB-04. It does
not establish full active-schema parity or AML feature parity: the other three
adapters still use legacy writers/readers. DB-05 through DB-09, explicit history
conversion/cutover, backup restore, real-chain acceptance and rollout remain open.
No ING-04 replay work or production migration was performed.
