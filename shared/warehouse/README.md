# Shared ClickHouse warehouse

All four adapters depend on this crate by path. `schema/0001_warehouse.json` is
the immutable baseline of the `aml_*` tables and views. Version 2 adds the
column rules in `schema/0002_column_contract.json`; `current_contract()` combines
them, while `baseline_contract()` returns the unchanged historical definition. The contract
contains column documentation, order/partition keys, engines, indexes and view
queries; the Rust migration runner embeds it directly. Database and network
identity are deployment parameters, never four separate schema copies.

This release installs the baseline and an **ordered v1-to-v2 column upgrade**.
It retains the same 29 tables, 20 views and physical columns and adds 217 rules
as one CHECK per table, alongside Rust value/type validation. Bitcoin's core writer and API
readers now use this contract; its old history requires explicit bounded
backfill. The other three adapters still use legacy writers/readers. Installing
the baseline does not backfill data or complete the four-network migration.
See [migration status and mapping](../../docs/warehouse/README_FA.md)
and [the remaining work](../../TODO_BITCOIN.md).

`initialize(connection, database, network)` validates metadata and history, then
checks existing shared rows before upgrading v1. Invalid history stops the
upgrade without changing rows or installing constraints. Fresh installs retain
both migration records; the identity points to v2 with a monotonic revision.
Restarting an interrupted initialization or constraint upgrade is supported.
Missing objects in an applied baseline, drift, an unknown migration,
an edited checksum or a conflicting network fail without silently replacing
objects. There is no DROP, TRUNCATE or automatic legacy conversion.

`validate(...)` uses SELECT and EXPLAIN only. It checks ledger/identity, names,
column order/types/defaults, engines, keys, partitions, settings, indexes and
view predicates and constraint expressions. `validate_identity(...)` is a smaller readiness dependency
check, not a substitute for full schema validation.

Do not edit an applied contract. The v1 checksum remains unchanged. The current
runner recognizes exactly v1 and v2, rejects unknown or changed history and
does not bless missing or modified constraints after v2 was recorded.

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
# SQL قابل بررسی در adapter

بیت‌کوین SQL پایه و `sql/migrations/20261026_0002_column_contract.sql` را از
قرارداد مشترک تولید و در schema binary مصرف می‌کند.
`migration::initialize_from_migrations` انطباق هر دو فایل را پیش از DDL کنترل
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

