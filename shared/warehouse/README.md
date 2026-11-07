# Shared ClickHouse contract

This Rust crate owns the `aml_*` schema and shared value, publication, query and
visibility rules for TRON, Ethereum, BSC and Bitcoin. It is a library, not a
separate service or central database. Each network VM has its own ClickHouse,
database and volumes.

`current_contract()` is the operational **v6** contract: **42 tables, 56 views,
739 physical columns**. DDL phases v1–v6 cover the baseline, column rules, core
evidence, analytics/specialized facts and visibility. Their SQL is generated
from the same embedded definitions. The new `20261106_0006_tron_evidence` adds
TRON receipt logs, internal records, nullable resource observations and contract
byte/JSON proof context. Earlier v1–v5 checksums and SQL stay frozen. This is
separate from the removed legacy-history v6–v10 lineage and its import planners;
old test volumes from that lineage are not upgraded.

The shared live entry point is `core::publish_observed_batch`. It validates the
whole batch before IO, rejects duplicate storage identities, verifies transaction
membership/position and network-specific fact families, then reserves a revision,
writes facts, reads back every column, completes the block and publishes its
checkpoint. `core::read` uses the same schema, exact integer decoding and a stable
checkpoint. Callers must own the network cursor and allocate a fresh revision
with `publication::next_revision`; this library is not a distributed writer lock.
Fact verification reads ordered pages of at most 256 rows so large Bitcoin
blocks do not require a single unbounded response. Every column is still
compared, and an extra row detects unexpected persisted evidence. Embedded
contract construction is cached per process; callers receive owned copies.
Database schema, ledger and checkpoint checks still read live state.
All four networks delegate their live block writers to this path. Bitcoin retains
UTXO conservation/prevout validation. Ethereum retains original EVM payloads,
log/trace positions and block-level withdrawal evidence. `refresh_observed_batch`
can improve coverage on an existing block without replacing its header, lowering
known coverage or moving the tip backward. Callers still require exclusive cursor
ownership. BSC also uses shared rollback, replay/repair, metadata and operational
records. TRON prepares a whole block in memory, preserving verified contracts,
receipt logs/internals/resources and exact transfer positions. Its publisher owns
both facts and cursor; replay refreshes an existing range without moving the tip.
TRON readers use query-local DTO projections. Its governed operational records,
metadata observations and sealed cluster/exposure outputs use the same tables.
Legacy DDL/mounts have been removed. All four schema jobs initialize the same
contract before runtime startup; one [generated SQL bundle](sql/README.md) is
kept for review and checked against the source.

Fresh databases are required. No automatic downgrade of an old test volume is
implemented. Do not edit an applied schema to match an existing volume.

All four networks use the shared core and operational records. BSC retains its
on-demand exposure analysis and central risk assessment. TRON retains reviewed
structural candidates and sealed observed exposure paths, with no fund-continuity
claim. TRON local ML/snapshot persistence has been retired in favor of the main
VM investigation service. This adds no shared table or service. No adapter-local migration copies remain. Full pipeline and VM acceptance is
tracked separately in [TODO_LIST](../../TODO_LIST.md).

```bash
cargo test --manifest-path shared/warehouse/Cargo.toml
cargo run --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --contract-json docs/warehouse/current-contract.json
```

The exporter supports baseline SQL, `--upgrade`, `--core`, `--analytics`,
`--visibility`, `--tron-evidence`, `--relationships`, `--contract-json` and `--check`.
See [schema guide](../../docs/warehouse/README_FA.md),
[current audit](../../docs/PROJECT_STATUS_FA.md) and
[testing](../../docs/TESTING.md).

TRON source verification is deliberately narrow: the original protobuf bytes
match txID; supported TransferContract, TransferAssetContract and
TriggerSmartContract financial fields match those bytes. Other named contracts
keep original Any bytes/JSON with `parameter_fields_verified=0`. Receipt
id/height/time linking is an RPC consistency check, not signature, inclusion
or consensus verification. `aml_tron_internals.call_value_info_json` retains
the complete ordered source array, including zero/omitted/rejected entries.
Raw failed-execution logs are evidence, never automatically successful flows.
