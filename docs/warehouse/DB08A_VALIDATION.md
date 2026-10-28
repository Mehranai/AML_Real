# DB-08A validation — 2026-10-27

DB-08A completes the tested backup/restore prerequisite. DB-08B/C/D and overall
DB-08 remain open. No runtime cutover, production backup, VM deployment, source
row conversion or schema migration was performed. Schema v5 is unchanged:
39 tables, 53 views, 682 physical columns, checksum
`8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`.

## Acceptance

Two task-owned ClickHouse **23.8.16.16** servers had independent data and backup
volumes. Each network fixture contained its checked-in legacy DDL, runtime
migration ledger where applicable, the complete v5 shared contract, source
checkpoint, migration/identity rows and an extra adversarial value table.
These are small synthetic fixtures, not copies of production databases.

| Network | Legacy objects | Shared objects | Edge fixture | Total objects |
|---|---:|---:|---:|---:|
| TRON | 37 | 92 | 1 | 130 |
| Ethereum | 39 | 92 | 1 | 132 |
| BSC | 30 | 92 | 1 | 123 |
| Bitcoin | 11 | 92 | 1 | 104 |

`scripts/tests/warehouse_backup_integration.py` exercised native asynchronous
BACKUP, copied each archive to the second server's independent volume, restored
into a previously absent database, and compared exact schemas and all physical
rows against the captured source. New client state resumed the recorded backup
job UUID. The test also checked:

- Two separately inserted revisions of the same ReplacingMergeTree key survive
  the backup and verification; source fixture and new target merges stay paused.
- Two UInt256 MAX values sum exactly above UInt256 MAX. One satoshi remains `1`;
  the other NULL value stays unknown. No float conversion is involved.
- Invalid UTF-8, binary strings, ordered arrays, signed zero, NaN payload bits,
  DateTime64 milliseconds, DateTime, Bool and signed integers are preserved.
- Legacy checkpoint fields and ledger/checksum rows, shared identity/history and
  uninitialized publication state are included in all-column verification.
- TRON's seeded transfer produces two materialized balance-delta rows. Restore
  retains exactly two; the MV does not apply the transfer a second time.
- Canonical views remain empty for an uninitialized shared publication. Restore
  does not publish legacy checkpoint contents as shared canonical facts.
- Existing target databases are refused. Changes to the live source after backup
  do not affect restored data. Merge release verifies only the isolated target.
- Offline receipts bind the exact backup manifest; `cutover_allowed` remains false.

[db08a-acceptance.json](db08a-acceptance.json) records sanitized per-network UUIDs,
counts and content digests. Full manifests/archives remain ignored under
`backups/`; credentials and production data are absent from the public receipt.

## Implementation findings and regression checks

Pinned 23.8 rejects a trailing FORMAT clause on BACKUP and uses the setting
`allow_different_table_def`, not `allow_different_table_structure`. Its DATABASE AS
restore can lose view table aliases. The controller restores empty structure,
validates every physical table, repairs only the new target's view metadata from
captured DDL, then restores data with nonempty-table rejection and verifies again.
These findings came from disposable server probes. No deployed view was changed.

The Python suite includes adversarial controller tests for uncertain dispatch,
both restore phases, missing server status, source changes, malformed manifests,
schema/row mismatch, target overwrite refusal and exact encoding. It checks that
an uncertain response does not produce a duplicate native data job. All **32**
backup unit tests pass; the complete **102-test** `scripts/tests/test_*.py` suite
and the offline dictionary check pass.
VM archive tests also verify that backup tooling/config is allowlisted alongside
the network runtime; backup data is not bundled. Compose rendering uses only
fixture environment values and confirms project-scoped independent backup disks.

Native job continuity requires system.backups history on the same server. Server
restart with lost job state fails closed. Replicated engines/Keeper, production
scale, failover and actual runtime parity remain outside this acceptance. Target
merge release ends the frozen-row verification window; the receipt is historical.
The backup disk is local to its VM and must be copied off-host for VM-loss recovery.

No Rust source changed in DB-08A, so this step does not claim a new Cargo test run.
Task-owned test containers and volumes are removed after collecting the receipts;
existing Docker networks/build caches and all real environments are retained.

Operator steps and limits: [DB08A_BACKUP_RESTORE_FA.md](DB08A_BACKUP_RESTORE_FA.md).
