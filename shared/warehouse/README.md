# Shared ClickHouse warehouse

All four adapters depend on this crate by path. `schema/0001_warehouse.json` is
the immutable baseline of the `aml_*` tables and views. Version 2 adds the
column rules in `schema/0002_column_contract.json`. Version 3 adds core evidence
fields from `schema/0003_core_evidence.json`. Version 4 adds analytics/specialized
contracts in `schema/0004_analytics_specialized.json`. Version 5 adds source
visibility in `schema/0005_visibility.json`; `current_contract()` combines all
five. `baseline_contract()`, `column_contract()`, `core_contract()` and
`analytics_contract()` return the frozen v1–v4 definitions. The contract
contains column documentation, order/partition keys, engines, indexes and view
queries; the Rust migration runner embeds it directly. Database and network
identity are deployment parameters, never four separate schema copies.

This release installs an **ordered v1-through-v5 upgrade**, with 39 tables,
53 views and 682 physical table columns. V2 adds 217 value rules; v3 appends
28 evidence/operation fields, seven core constraints and a benchmark-current view.
V4 adds nine tables and 110 columns on existing tables for provenance, discovery,
protocol/entity catalogs, pair claims, exposure/risk detail and specialized evidence.
Bitcoin's core writer and API
readers now use this contract; its old history requires explicit bounded
backfill. The other three adapters still use legacy writers/readers. Installing
the baseline does not backfill data or complete the four-network migration.
See [migration status and mapping](../../docs/warehouse/README_FA.md)
and [the remaining work](../../TODO_BITCOIN.md).

DB-08A adds an opt-in native backup and isolated restore controller in
`scripts/warehouse_backup.py`, with independent project-scoped backup disks for
each network VM. It captures legacy and shared objects together, verifies exact
schemas/all physical rows, and never authorizes a runtime cutover. This does not
change schema v5 or complete DB-08B/C/D. See the
[operator runbook](../../docs/warehouse/DB08A_BACKUP_RESTORE_FA.md) and
[acceptance](../../docs/warehouse/DB08A_VALIDATION.md).

`initialize(connection, database, network)` validates metadata and history, then
checks existing shared rows before each upgrade. Invalid history stops the
upgrade without changing rows or installing constraints. Fresh installs retain
all five migration records; the identity points to v5 with a monotonic revision.
Restarting interrupted column, constraint and view upgrades is supported.
Missing objects in an applied baseline, drift, an unknown migration,
an edited checksum or a conflicting network fail without silently replacing
objects. There is no DROP, TRUNCATE or automatic legacy conversion.

`validate(...)` uses SELECT and EXPLAIN only. It checks ledger/identity, names,
column order/types/defaults, engines, keys, partitions, settings, indexes and
view predicates and constraint expressions. `validate_identity(...)` is a smaller readiness dependency
check, not a substitute for full schema validation.

Do not edit an applied contract. The v1 checksum remains unchanged. The current
runner recognizes exactly v1 through v5, rejects unknown or changed history and
does not bless missing or modified applied constraints, defaults, columns or views.

`core` provides bounded network-scoped readers for blocks, transactions,
relationships, features, events, state, failures and benchmarks. `publish_batch`
checks fact headers/counts and publishes through the existing single-owner fence;
Bitcoin also verifies every stored fact value before completion. Missing count
keys and nullable failure timestamps remain unknown. Source JSON, event sub-index,
ordered trace paths and exact NFT token IDs survive the v3 contract. See
[DB-04 scope](../../docs/warehouse/DB04_CORE_CONTRACT_FA.md); legacy reader/writer
cutover and history conversion remain DB-08.

`analytics` supplies bounded network-scoped reads for the analytics families,
with explicit `Current` and supported `Approved` projections. Approved claims,
protocols and entity/exchange addresses depend on reviewed evidence and active
sources; a later revocation supersedes earlier approval. Current run/path/risk
rows use the DB-07 sealed visibility boundary for current reads. Specialized facts
(discoveries, scripts, traces and raw envelopes) share the exact block fence.
Creating these tables does not implement the Bitcoin AML workers or APIs. See
[DB-05 scope](../../docs/warehouse/DB05_ANALYTICS_CONTRACT_FA.md).

Bitcoin now uses the existing intelligence source/claim/review tables through
its operator CLI and investigation API. Active attribution additionally requires
a matching published review revision; the adapter supplies an address-scoped
intelligence snapshot to the central freshness check. No schema or scoring weight
was added for this path. Other Bitcoin analytics workers remain open. See
[reviewed attribution](../../dockerizd_bitcoin/INTELLIGENCE.md).

`relationships` provides the versioned `typed_relationships_v1` interpretation
and a bounded canonical graph reader for all four networks. Account transfers
connect two address subjects. Bitcoin records separate prevout-to-transaction
and transaction-to-output legs, including addressless scripts; no input value
is allocated to individual outputs. Each Bitcoin leg must match its canonical
vin/vout fact, amount, position, subject and outpoint. Transaction occurrence
keys include the block/commit/revision; a changed publication fence fails the
read. Non-address nodes have NULL addresses. Bitcoin uses this reader and writer
validation; active legacy reader/writer cutover for the other networks remains
DB-08. Its interpretation remains compatible with v5; applied v4 stays frozen.
See [DB-06](../../docs/warehouse/DB06_RELATIONSHIPS_FA.md).

The reviewable SQL template is `src/relationships/query.sql`. Generate/check
the Bitcoin copy (a query artifact, not a migration) from the repository root:

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --relationships --check dockerizd_bitcoin/sql/queries/relationships.sql
```

The template placeholders are rendered by the shared reader from validated
identifiers, escaped literals, exact occurrence tuples and bounded integers.
Do not execute an unrendered template or interpolate user input into it.

`visibility::capture` produces a source snapshot; `publish_analysis` writes and
verifies an immutable analysis attempt before sealing its output revision.
Cluster versions, exposure aggregates and risk children share their parent
seal. Chain metadata requires observed hash/source revision, and chain-discovered
jobs require canonical discovery evidence. `analytics::Mode::Current` uses
`_visible` for these tables; `Mode::Audit` is explicitly historical access.
Any source fence change invalidates old seals conservatively. An explicit
`invalidate_analysis` appends revocation without deleting historical evidence.
See [DB-07](../../docs/warehouse/DB07_VISIBILITY_FA.md) for boundaries and the
remaining legacy cutover. Confirmation depth is not consensus finality.

`publication` supplies the common block reservation, fact-header validation,
complete marker and final publication fence. Each JSON row explicitly supplies
all contract columns with exact JSON types. UInt256 amounts require canonical
decimal strings; hashes, native precision, asset identity, reviews, confidence,
coverage and evidence references follow the shared rules. Validation of the
entire row batch precedes database I/O. A reserved revision prevents retrying a partial write
with the same revision after a crash or clock rollback. Exact fact identity is
network/height/hash/commit/revision. The Bitcoin adapter verifies all persisted
core columns before completing and publishing a block. These helpers require
a single cursor owner; their stale-check is not a distributed writer lease.

## Build and check

Build with the network's Compose file, which supplies the `warehouse_source`
named build context. For a direct build from the repository root, for example:

```bash
docker build --build-context warehouse_source=./shared/warehouse \
  -f dockerizd_bitcoin/Dockerfile dockerizd_bitcoin
```

Both normal and offline Dockerfiles use this source context. Offline dependency
archives must be refreshed after the Cargo.lock change. Existing runtime-only
VM bundles use prebuilt images with `--no-build`.

The `.dockerignore` in this directory allows only Cargo files, Rust sources and
the schema. Never use the whole repository as the shared build context.

```bash
cargo test --manifest-path shared/warehouse/Cargo.toml
cargo clippy --manifest-path shared/warehouse/Cargo.toml --all-targets -- -D warnings
bash scripts/vm.sh bitcoin check-schema
# The same action supports tron, ethereum and bsc.
```

## Integration tests

Use an empty disposable ClickHouse 23.8 with the fixture-only user
`warehouse_test` / `warehouse-test-only`. The test creates fixed
`warehouse_test_*` databases, deliberately changes some fixture definitions and
refuses nonempty fixtures. It never deletes or repairs an existing database.

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:8123 \
  cargo test --manifest-path shared/warehouse/Cargo.toml \
  --test clickhouse_contract -- --ignored
```

Native-driver checks in `dockerizd_bsc/tests/shared_warehouse.rs` and
`dockerizd_tron/app/tests/shared_warehouse.rs` use the same fixture credentials.
They verify the two driver versions and full validation with `readonly=1`.

`relationship_contract` adds four-network query fixtures, multi-input/output
and script evidence, repeated transaction hashes in separate blocks, limits,
missing/mismatched witnesses and a moving-snapshot check. Its integration tests
require fresh `db06_graph_*` and `db06_occurrences` fixture databases:

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:8123 cargo test \
  --manifest-path shared/warehouse/Cargo.toml --test relationship_contract -- --include-ignored
```
# SQL قابل بررسی در adapter

بیت‌کوین SQL پایه و migrationهای نسخهٔ ۲، ۳ و ۴ را از
قرارداد مشترک تولید و در schema binary مصرف می‌کند.
`migration::initialize_from_all` انطباق هر چهار فایل را پیش از DDL کنترل
می‌کند و همان مسیر مشترک ledger، identity، preflight و drift را اجرا می‌کند.
تعریف‌ها فقط در بستهٔ مشترک‌اند؛ SQLها قرارداد مستقلی نیستند.

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --check dockerizd_bitcoin/sql/migrations/20261022_0001_shared_warehouse.sql
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --upgrade --check dockerizd_bitcoin/sql/migrations/20261026_0002_column_contract.sql
```

The value grammar, upgrade boundaries and remaining cutover work are described
in [DB-03](../../docs/warehouse/DB03_COLUMN_CONTRACT_FA.md).

