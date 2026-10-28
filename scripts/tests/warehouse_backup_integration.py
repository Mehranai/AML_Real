#!/usr/bin/env python3
"""Disposable DB-08A native backup/restore proof for all four project schemas.

Requires two fresh ClickHouse 23.8 containers configured with backup_disk.xml.
Creates only aml_inventory_* / db08a_restore_* fixtures; never drops databases.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time
import urllib.parse

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import warehouse_backup as backup
import warehouse_inventory as inventory


def common_schema(client, database, network):
    contract = json.loads((ROOT / "docs/warehouse/current-contract.json").read_text(encoding="utf-8"))
    checksum = hashlib.sha256(json.dumps(contract, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
    for table in contract["tables"]:
        columns = ["`" + c["name"] + "` " + c["data_type"] + (" DEFAULT " + c["default"] if c["default"] is not None else "") for c in table["columns"]]
        columns.extend(f"INDEX {i['name']} {i['expression']} TYPE {i['index_type']} GRANULARITY {i['granularity']}" for i in table.get("indexes", []))
        columns.extend(f"CONSTRAINT `{c['name']}` CHECK {c['expression']}" for c in table.get("constraints", []))
        partition = " PARTITION BY " + table["partition_by"] if table["partition_by"] else ""
        client.execute(f"CREATE TABLE {database}.{table['name']} ({','.join(columns)}) ENGINE={table['engine']}{partition} ORDER BY ({','.join(table['order_by'])}) SETTINGS index_granularity=8192")
    for view in contract["views"]:
        client.execute(f"CREATE VIEW {database}.{view['name']} AS " + view["select"].replace("{{database}}", database))
    def row(table, value):
        client.execute(f"INSERT INTO {database}.{table} FORMAT JSONEachRow\n" + json.dumps(value))
    identity = {"network_id": backup.NETWORKS[network], "schema_version": contract["version"], "checksum": checksum, "revision": 1, "created_at_unix_ms": 1}
    row("aml_warehouse_identity", identity)
    history = json.loads((ROOT / "docs/warehouse/db07-acceptance.json").read_text())["databases"][0]["migration_history"]
    ids = {1: "20261022_0001_shared_warehouse", 2: "20261026_0002_column_contract", 3: "20261026_0003_core_evidence", 4: "20261027_0004_analytics_specialized", 5: "20261027_0005_visibility"}
    for migration in history:
        row("aml_schema_migrations", {**identity, **migration, "migration_id": ids[migration["schema_version"]], "revision": migration["schema_version"]})
    row("aml_sync_state", {"network_id": backup.NETWORKS[network], "revision": 1, "initialized": 0, "start_block": 0,
        "last_block_number": 0, "last_block_hash": "", "observed_rpc_height": 0, "published_at_unix_ms": 1, "source_cursor_json": "{}"})


def seed(client, database, network):
    if network in ("ethereum", "bsc"):
        source = (ROOT / inventory.CHAINS[network][0] / "src/db/migrations.rs").read_text(encoding="utf-8")
        ledger = re.search(r'CREATE TABLE IF NOT EXISTS \{database\}\.schema_migrations\s*\([\s\S]*?ORDER BY migration_id', source)
        if not ledger:
            raise ValueError("Runtime legacy migration ledger was not found")
        client.execute(ledger.group().replace("{database}", database))
        client.execute(f"INSERT INTO {database}.schema_migrations (migration_id,description,checksum) VALUES ('fixture-only','restore proof','fixture-checksum')")
    checkpoint = {
        "tron": ("sync_state", {"chain": "tron", "last_synced_block": 7}),
        "ethereum": ("sync_state", {"network_id": backup.NETWORKS[network], "last_synced_block": 7, "last_synced_block_hash": "a"*64, "updated_at_unix_ms": 123000}),
        "bsc": ("sync_state", {"network_id": backup.NETWORKS[network], "next_block": 8, "last_finalized_block": 7, "last_finalized_block_hash": "a"*64, "state_revision": 2, "updated_at_unix_ms": 123000}),
        "bitcoin": ("bitcoin_sync_state", {"id": 1, "revision": 2, "initialized": 1, "start_height": 7, "tip_height": 7, "tip_hash": "a"*64, "observed_rpc_height": 8, "published_at": 123000}),
    }[network]
    client.execute(f"INSERT INTO {database}.{checkpoint[0]} FORMAT JSONEachRow\n" + json.dumps(checkpoint[1]))
    maximum = str(2**256 - 1)
    client.execute(f"CREATE TABLE {database}.backup_edge_values (k UInt64,revision UInt64,amount UInt256,value_sats Nullable(UInt64),a Array(String),positions Array(UInt32),f32 Float32,f64 Float64,raw String,flag Bool,ts DateTime64(3,'UTC'),seen DateTime,n Int8) ENGINE=ReplacingMergeTree(revision) ORDER BY k")
    client.execute(f"SYSTEM STOP MERGES {database}.backup_edge_values")
    client.execute(f"INSERT INTO {database}.backup_edge_values VALUES "
        f"(1,1,toUInt256('{maximum}'),1,['x',unhex('ff')],[1,0,2],toFloat32(0.1),reinterpretAsFloat64(unhex('0000000000000080')),unhex('ff000a'),true,toDateTime64('2020-01-01 12:34:56.789',3,'UTC'),toDateTime(123),-1)")
    client.execute(f"INSERT INTO {database}.backup_edge_values VALUES "
        f"(1,2,toUInt256('{maximum}'),NULL,[unhex('ff'),'x'],[2,0,1],reinterpretAsFloat32(unhex('00000080')),reinterpretAsFloat64(unhex('010000000000f87f')),unhex('ff000b'),false,toDateTime64('2020-01-01 12:34:56.790',3,'UTC'),toDateTime(124),1)")
    if network == "tron":
        client.execute(f"INSERT INTO {database}.address_relationships (relationship_id,from_address,to_address,token_address,tx_hash,block_number,timestamp,amount,transfer_type,inserted_at) VALUES ('fixture','T1111111111111111111111111111111111','T2222222222222222222222222222222222','TRX','{'a'*64}',7,123000,1,'transfer',now64(3))")
    # Keep physical revision history stable during this isolated restore proof.
    for item in client.rows("SELECT name FROM system.tables WHERE database=" + backup.literal(database) + " AND engine NOT IN ('View','MaterializedView')"):
        client.execute(f"SYSTEM STOP MERGES `{database}`.`{backup.identifier(item['name'])}`")


def wait(client, manifest, timeout=180):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = backup.refresh(client, manifest)
        if value["status"] in ("verified", "rejected"):
            return value
        time.sleep(.15)
    raise TimeoutError("Native job did not finish before the integration deadline")


def copy_archive(container, destination, archive, folder):
    local = folder / archive
    subprocess.run(["docker", "cp", f"{container}:/var/lib/clickhouse/backups/{archive}", str(local)], check=True)
    subprocess.run(["docker", "cp", str(local), f"{destination}:/var/lib/clickhouse/backups/{archive}"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-url", required=True)
    parser.add_argument("--restore-url", required=True)
    parser.add_argument("--disposable", action="store_true", required=True)
    args = parser.parse_args()
    for url in (args.source_url, args.restore_url):
        if urllib.parse.urlsplit(url).hostname not in ("127.0.0.1", "localhost"):
            parser.error("Integration endpoints must be disposable localhost services")
    source = backup.Client(args.source_url, "warehouse_test", "warehouse-test-only", timeout=120)
    restored = backup.Client(args.restore_url, "warehouse_test", "warehouse-test-only", timeout=120)
    legacy = inventory.inventory(inventory.TestClickHouse(args.source_url, "warehouse_test", "warehouse-test-only"))
    folder = ROOT / "backups" / ("db08a-" + str(time.time_ns()))
    folder.mkdir(parents=True, exist_ok=True)
    results = []
    for network, info in legacy["chains"].items():
        database, target = info["database"], "db08a_restore_" + network
        common_schema(source, database, network)
        seed(source, database, network)
        before = backup.stable_snapshot(source, database, network)
        manifest, receipt = folder / (network + "-backup.json"), folder / (network + "-restore.json")
        first = backup.backup(source, database, network, manifest, paused=True)
        first = wait(source, manifest)
        assert first["status"] == "verified", backup.public_status(first)
        # New controller process/client state resumes the known operation ID.
        assert backup.backup(backup.Client(args.source_url, "warehouse_test", "warehouse-test-only"), database, network, manifest, paused=True, resume=True)["operation_id"] == first["operation_id"]
        copy_archive("aml-db08a-source", "aml-db08a-restore", first["archive"], folder)
        backup.restore(restored, manifest, target, receipt)
        second = wait(restored, receipt)
        assert second["status"] == "verified", backup.public_status(second)
        proof = backup.compare_receipts(manifest, receipt)
        actual = backup.stable_snapshot(restored, target, network)
        assert actual == before
        edge = next(t["data"] for t in actual["objects"] if t["name"] == "backup_edge_values")
        assert edge["rows"] == 2  # All physical revisions, not only FINAL.
        assert edge["exact_amount_totals"][0]["total"] == str(2*(2**256-1))
        assert edge["exact_amount_totals"][1]["total"] == "1"
        assert edge["exact_amount_totals"][1]["null_rows"] == 1
        if network == "tron":
            deltas = next(t["data"] for t in actual["objects"] if t["name"] == "wallet_asset_balance_deltas_v3")
            assert deltas["rows"] == 2, "Restoring MVs must not double-apply transfers"
        canonical = restored.rows(f"SELECT toString(count()) AS n FROM {target}.aml_transactions_canonical")[0]["n"]
        assert canonical == "0", "Restore must not publish an uninitialized warehouse"
        try:
            backup.restore(restored, manifest, target, folder / (network + "-overwrite.json"))
        except ValueError:
            pass
        else:
            raise AssertionError("Existing restore target was accepted")
        source.execute(f"INSERT INTO {database}.backup_edge_values (k,revision,amount,raw) VALUES (2,3,1,'after backup')")
        assert backup.stable_snapshot(restored, target, network) == before, "Restore must not read live source rows"
        assert backup.stable_snapshot(source, database, network) != before
        assert backup.release_restored_merges(restored, receipt)["merges"] == "resumed"
        results.append(proof)
        print("PASS native backup/restore:", network, proof["objects"], "objects", flush=True)
    report = {"scope": "DB-08A disposable legacy+shared-v5 snapshots; no runtime cutover", "clickhouse_version": "23.8.16.16", "row_encoding": backup.ROW_ENCODING,
              "shared_schema_version": 5, "shared_schema_changed": False, "cutover_allowed": False,
              "tests": ["exact all-column multiset", "unbounded integer totals", "binary/float bits", "nullable values", "array order", "timestamps", "all physical revisions", "legacy checkpoints and migration ledgers", "materialized-view no double count", "source isolation after backup", "existing target refusal"], "networks": results}
    (ROOT / "docs/warehouse/db08a-acceptance.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
