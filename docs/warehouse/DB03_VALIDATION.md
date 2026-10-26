# DB-03 validation — 2026-10-26

The shared value contract and v1-to-v2 upgrade are implemented and verified.
This completes DB-03 for the shared `aml_*` contract. Legacy writer/reader
cutover, complete database parity and production rollout remain open.

## Implemented scope

- Same 29 tables, 20 views and physical columns as v1; 217 immutable column
  rules generate one `aml_columns_v2` CHECK per table and Rust row validation.
- Exact UInt256 decimal strings, checked decimal conversion, native precision,
  network/hash/asset identity, NFT token identity, review states, finite
  confidence/scores, JSON/hex, namespaced evidence and nullable capabilities.
- All input rows are validated before any insert batch or identity query.
- Bitcoin uses common hash/native precision helpers. Its transaction amount and
  sender/recipient remain NULL; observed UTXO legs retain exact satoshis.
- Both baseline and upgrade SQL are embedded and checked before Bitcoin DDL.
- Existing shared history is preflighted before ALTER/ledger updates. No rows
  are rewritten. Applied baseline objects cannot be silently recreated empty.
- Interrupted upgrades resume only when overlapping constraints match. Applied
  constraint drift is rejected; ledger/identity checks recognize both versions.
- Identity revision is monotonic even after clock rollback; exhaustion stops
  upgrade before ALTER. `--check` remains SELECT/EXPLAIN only.

## Results

| Check | Result |
|---|---|
| Shared Rust crate | **19 passed**: 7 unit, 7 value-contract, 5 real ClickHouse integration |
| Bitcoin Rust | **53 passed**: 10 unit, 2 pipeline, 23 resilience, 16 probe, 2 mapping; includes 14 ClickHouse tests |
| Native SDK compatibility | **2 passed**, ClickHouse client 0.14.1 and 0.14.3, including explicit bounded reads and `readonly=1` validation |
| Python launcher/provisioner/dictionary | **61 passed** |
| Strict Clippy | Shared crate and Bitcoin, `--all-targets -- -D warnings`, passed |
| Generated SQL | v1 and v2 `export_sql --check` passed |
| Actual dedicated schema binary | `bitcoin_init_schema --check` returned version 2, 29 tables, 20 views, expected checksum, `newly_applied=false` |
| Dictionary | Regenerated with v2 rule identity/hash; offline `--check` passed |

The native tests use the checked-in `tests/shared_warehouse.rs` sources. Final
driver checks also ran in temporary minimal Cargo harnesses with exactly
0.14.1/native-tls and 0.14.3/default features, avoiding repeated linking of all
unrelated application binaries. No deployment manifest or production dependency
version was changed for those harnesses.

The complete Bitcoin run initially exposed an old test fixture using
`network_id='ethereum'`. It was changed to canonical `eip155:1`, and the same
protected/filtering/pagination test passed on rerun. The other resilience tests
passed; probe/mapping suites were then completed separately. Validation rules
were not relaxed to accommodate the fixture.

## Real database evidence

ClickHouse image `clickhouse/clickhouse-server:23.8` returned **23.8.16.16**.
Fresh and upgraded disposable databases were used, with explicit fixture-only
credentials and tmpfs storage; no project volume or `.env` was mounted.

| Upgraded fixture | Checked tables | Views |
|---|---:|---:|
| db03_upgrade_tron | 29 | 20 |
| db03_upgrade_ethereum | 29 | 20 |
| db03_upgrade_bsc | 29 | 20 |
| db03_upgrade_bitcoin | 29 | 20 |

All normalized table CREATE definitions, including constraints, matched across
the four databases. Existing records in every applicable table except the two
schema bookkeeping tables were compared byte-for-byte as JSON before/after
upgrade. Both migration records were retained, and the winning identity became
v2 despite a deliberately far-future v1 revision.

Negative cases covered incompatible v1 confidence, a missing applied table,
exhausted identity revision, partial ALTER completion, removed/modified applied
constraints, malformed hashes/JSON/evidence, wrong native precision, missing
review actor, invalid review enum, Bitcoin single-sender/amount fabrication,
non-applicable coverage and outpoint-index overflow. Invalid history and missing
objects remained intact; no empty replacement table or rewritten evidence was
created. Shared publication tests still verified invisible partial commits,
canonical fences, network filtering, restart and revision allocation.

## Driver issues resolved

Both native clients treat `?` as a bind marker even inside string literals.
`Connection` receives fully rendered SQL, so the shared native adapter escapes
every literal `?` as `??` before giving SQL to the client. Tests preserve single,
double and `?fields` literal sequences, including the regexes used by CHECKs.

Client 0.14.1 additionally forces `readonly=1` on normal fetches, which rejects
query-local resource SETTINGS. The dedicated `query_bounded` preflight method
uses `readonly=2` for that SELECT only: writes remain prohibited, while timeout
and thread limits are permitted. The original client and full `--check` path
keep their configured mode; `readonly=1` validation passed and initialization
through that client was rejected as expected. Both versions explicitly tested
the bounded read with a literal question mark and resource settings.

## Immutable identities

- v1 file SHA256: `0ae654956ac2673cd67be2223b97e545612875007ffc1c57b2dec838b8d3c483`.
- v1 contract checksum: `147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`.
- v2 current contract checksum: `c76d93f7c782fc09b7a87d5b7a0337203b595ca60fcd3fc6ca85092c2bb9b5f5`.
- v2 migration id: `20261026_0002_column_contract`.

## Reproduction and remaining scope

Use new disposable ClickHouse fixtures only; tests deliberately retain drift
fixtures and refuse reused databases where required.

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:8123 \
  cargo test --locked --manifest-path shared/warehouse/Cargo.toml --all-targets -- --include-ignored
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
  cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml --all-targets -- --include-ignored
WAREHOUSE_TEST_URL=http://127.0.0.1:8123 \
  cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test shared_warehouse -- --ignored
WAREHOUSE_TEST_URL=http://127.0.0.1:8123 \
  cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --test shared_warehouse -- --ignored
python scripts/warehouse_data_dictionary.py --check
```

These results do not exercise live chain RPC, production migration, release
images, five-VM rollout, backup/restore, analytics parity, browser/Neo4j acceptance
or legacy adapter cutover. Those remain in DB-04 through DB-09 and P1–P3. The
new CHECKs preserve v1 columns; DB-01's unmapped analytical/specialized fields
still require later contracts/backfill. ING-04 remains deferred.

All 47 staged source/schema/SQL/web files matched the delivered workspace bytes;
documentation was checked separately. Rustfmt and whitespace checks passed.
The two disposable ClickHouse containers and compiler container were removed;
build caches and the pre-existing Docker network were retained.
