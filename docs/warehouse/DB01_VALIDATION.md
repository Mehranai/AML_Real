# DB-01 validation — 2026-10-26

Scope: a reproducible DDL/source dictionary and a reviewed migration map.
No adapter writer/reader was switched and no production database was contacted.

## Materialized schema evidence

- Image: `clickhouse/clickhouse-server:23.8`; server returned `23.8.16.16`.
- A new `aml-db01-audit` container used tmpfs for database/log storage and an
  ephemeral localhost HTTP port. It had no project database volume or `.env`.
- Existing legacy SQL was applied in order to four empty `aml_inventory_*`
  databases. The actual code-defined Ethereum/BSC migration-ledger DDL was
  additionally materialized. The same shared generated SQL was then applied.
- The dictionary compares all shared object metadata across the four databases,
  including columns/defaults/codecs, view SQL, engines, partition/sort/primary
  keys and complete CREATE statements with only the disposable database name
  normalized. All 29 shared tables also match the source contract column order
  and types. Shared indexes and TTL are recorded from the CREATE definitions.

| Fixture database | Legacy objects | Legacy columns | Shared objects |
|---|---:|---:|---:|
| aml_inventory_tron | 37 | 453 | 49 |
| aml_inventory_ethereum | 39 | 575 | 49 |
| aml_inventory_bsc | 30 | 511 | 49 |
| aml_inventory_bitcoin | 11 | 113 | 49 |

The 49 shared objects comprise 29 tables / 376 columns and 20 views / 285 output
columns. These are schema counts, not row counts or feature-parity assertions.
The old Bitcoin objects were deliberately installed only as migration fixtures.

The immutable v1 schema file SHA256 remained:
`0ae654956ac2673cd67be2223b97e545612875007ffc1c57b2dec838b8d3c483`.
Its migration checksum remains
`147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`.

## Source and mapping checks

`python scripts/warehouse_data_dictionary.py --check` passed offline after
generation. Every legacy object/column has a disposition. Referenced target
fields exist in the common contract. Missing target inputs are listed separately.
Source manifests, DDL file lists/hashes and generic-dispatch anchors are checked.

Access evidence distinguishes literal SQL, helper inserts, generic bindings,
constant dispatch, DDL seed writes, materialized-view targets, view dependencies,
unproven references, and SQL-export producers. An early `cfg(test)` import in BSC
does not hide the active ingestion writer. Test-module tails are excluded.
This is a checked source audit, not a general Rust compiler or proof of live
worker deployment. Table-specific direct evidence and family call chains are
separate so a family worker does not imply a writer for every table in it.

Three existing TRON training-export schema conflicts were recorded (missing
training-run table, registry columns and deployment columns). The trainer and
its generated SQL were never executed. No Bitcoin ML capability was introduced.

## Automated results

The complete `scripts/tests/test_*.py` suite passed: **61 tests**, including
**17 new dictionary tests**. They cover:

- every object/column, existing target destinations and explicit numeric units;
- rejection of unknown mappings, missing/duplicate snapshot objects, new or
  removed DDL files, stale DDL hashes and missing network fixtures;
- shared shape drift and wrong fixture database identities;
- separate legacy/shared migration ledgers;
- Bitcoin satoshi output totals, second-to-millisecond conversion and newly
  allocated commit/revision semantics;
- generic writer dispatch, BSC writer visibility, Ethereum review helpers and
  seed-only policies, and DDL-triggered materialized writes;
- absent target publication inputs and stale owner evidence;
- installed-but-inactive analytics versus Bitcoin-not-applicable raw tables;
- incompatible SQL exports being reported without being run.

`git -c core.safecrlf=false diff-files --check` also passed. This change adds
audit tooling, tests and documentation; Rust runtime sources and applied schema
definitions were not changed, so Rust runtime suites were not repeated here.

## Reproduction and limits

```powershell
python scripts/warehouse_data_dictionary.py --check
python -m unittest discover -s scripts/tests -p test_warehouse_data_dictionary.py -v
```

For a fresh DDL snapshot, start a new disposable localhost ClickHouse with test
user `warehouse_test` / password `warehouse-test-only`, then run:

```powershell
python scripts/warehouse_data_dictionary.py --refresh --disposable --url http://127.0.0.1:PORT
```

The tool refuses nonempty legacy fixture databases and does not drop data.
The temporary container created for this run was removed after the audit.
Docker Desktop initially was unavailable; after the user started it, the real
ClickHouse checks above completed successfully.

Not exercised: application init/upgrade ledgers and network binding on these new
fixtures, live ingestion/RPC, semantic data conversion, row/amount reconciliation,
worker execution, restore, API/Neo4j acceptance, VM deployment or release images.
Those remain in DB-03 through DB-09 and the acceptance checklist. The results
close DB-01's inventory/source/mapping deliverable, not the complete DB migration.
