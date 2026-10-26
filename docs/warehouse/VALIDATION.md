# Shared warehouse validation — 2026-10-22

Scope: additive contract version 1, migration
`20261022_0001_shared_warehouse`, checksum
`147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`.
There are 29 tables, 20 views and 376 explicitly declared table columns.

The checks below use source-only staging directories, Rust 1.92 and disposable
ClickHouse 23.8 containers with fixture-only credentials. No deployed database,
private environment file or customer transaction history was used.

## Passed checks — baseline milestone

- Shared crate: 5 unit tests and `cargo clippy --all-targets -- -D warnings`.
  Tests cover exact UInt256 amounts/one satoshi, timestamp overflow, common DDL,
  safe identifiers, checksum stability and SQL normalization.
- `shared/warehouse/tests/clickhouse_contract.rs`: real ClickHouse test passed.
  It compares all four schema shapes; validates engines, keys, partitions,
  indexes and views; preserves a preexisting legacy evidence row; exercises
  idempotent and interrupted initialization; rejects network, checksum, column
  and view drift; checks unpublished facts and rollback visibility; runs the
  same canonical query for every database/network combination.
- All targets compile for BSC, Ethereum and TRON. Bitcoin's tests compile all
  its targets. Cargo.lock updates add only the local shared package and its
  dependency reference to the existing adapter lockfiles.
- Bitcoin: 7 unit tests, strict Clippy, and the real ClickHouse/RPC-fixture
  pipeline test passed after installing the common schema. The pipeline covers
  restart, exact UTXOs, graph/paths, unpublished facts and reorg. The actual
  `bitcoin_aml schema --check` command returned version 1 and the checksum above.
- BSC native ClickHouse 0.14.3 integration test: initialization and full
  validation with `readonly=1` passed. Attempting initialization with that
  read-only setting fails as expected. The actual `bsc_schema` and
  `bsc_schema --check` commands also passed with legacy and shared schemas in
  the disposable database.
- TRON native ClickHouse 0.14.1 integration test: initialization and full
  validation with `readonly=1` passed. Ethereum uses the same 0.14.1 driver;
  its own adapter was also checked with all targets. The actual `tron_schema`
  and `tron_schema --check` commands passed over the full legacy TRON baseline
  in the disposable server.
- Python: 44 launcher/provisioning/inventory tests passed; temporary files were
  confined to the workspace. Inventory tests cover SQL literals/comments,
  incomplete SQL, local fixture origins and database scope checks.
- Linux CLI contract suite passed, including all four `check-schema` routes.
  Docker, HTTP and Cargo are test doubles in that suite.
- Gateway routing: 6 Node tests passed.
- Compose configurations parsed with example settings. A real Docker build of
  the shared named context verified source/schema availability, exclusion of
  non-allowlisted files and offline Cargo metadata resolution.
- Legacy inventory was materialized from checked-in SQL on real ClickHouse:
  TRON 37 objects / 453 columns; Ethereum 38 / 571; BSC 29 / 507; Bitcoin
  11 / 113. Source references are audit leads, not runtime coverage proof.

## Bitcoin core migration milestone

Bitcoin's active core writer and readers now use the shared tables. Fresh
initialization creates no `bitcoin_*` objects. Existing legacy history is
converted only by the explicit bounded `warehouse-backfill` command; legacy
objects and rows are retained.

The expanded ClickHouse/RPC fixture checks resumable one-block migration,
idempotence, exact row/amount equivalence, typed transaction/input/output legs,
readiness during partial migration, rejection and repair of an incomplete
legacy block, an unexpected change of the source checkpoint even with the same
hash, missing legacy schema objects, stale publication, crashes before/after a complete marker, restart,
clock rollback and reorg. Completion receipts are tied to the frozen legacy
revision and remain usable after a legitimate later warehouse reorg.

The shared publication fixture checks all four network identities, every
required fact column, foreign headers, database binding at insert time,
UInt256 precision, unpublished facts, reserved revisions, stale publication
and rollback without deleting evidence. The native driver checks also insert
and round-trip the shared sync-state model before read-only validation.

These are disposable fixture migrations, not a migration of a deployed
database. The final execution results for this milestone are recorded below.

- Bitcoin: 7 unit tests, 2 mapping tests and both real ClickHouse integration
  tests passed on the final source; strict Clippy passed.
- Shared contract: 5 unit tests and both real ClickHouse integration tests
  passed; strict Clippy passed. The publication test uses the same helpers for
  TRON, Ethereum, BSC and Bitcoin.
- Native ClickHouse drivers 0.14.3 in BSC and 0.14.1 in TRON (also used by Ethereum): shared
  state insertion/round-trip and read-only schema validation passed.
- Actual Bitcoin CLI: `schema`, `schema --check` and
  `warehouse-backfill --max-blocks 1` passed on a fresh fixture database. The
  last command reported no legacy history to migrate; data conversion itself
  is exercised by the two integration tests, not that empty CLI invocation.
- All 44 Python launcher/provisioning/inventory tests passed. No dependency
  versions or applied schema/checksum changed in this milestone.

## Limits of this evidence

The TRON/Ethereum/BSC writers/readers and the analytical workers have not been
migrated to the new tables. The tests do not prove production backfill, all legacy
feature mappings, backup/restore, stale-analysis invalidation or AML/API/UI
parity. Those acceptance items remain open in [TODO](../../TODO_BITCOIN.md).

The Docker context check is not a release image build. No production RPC
benchmark, five-VM rollout, offline archive refresh or live migration was run.
The Ethereum schema CLI was checked by compilation; it was not executed.
The schema fixture checks additive installation over retained legacy evidence;
it does not certify conversion of a deployed historical warehouse.
