"""Opt-in acceptance of four real schema CLIs against four empty test servers.

Each --binary/--url takes NETWORK=VALUE. All servers use warehouse_test /
warehouse-test-only. This test never drops a database or reuses existing tables.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
DATABASES = {"tron": "tron_db", "ethereum": "ethereum_aml", "bsc": "bsc_aml", "bitcoin": "bitcoin_aml"}
NETWORKS = {"tron": "tron:mainnet", "ethereum": "eip155:1", "bsc": "eip155:56", "bitcoin": "bip122:000000000019d6689c085ae165831e93"}


def query(url, sql):
    request = urllib.request.Request(url, data=(sql + " FORMAT JSONEachRow").encode(), headers={
        "X-ClickHouse-User": "warehouse_test", "X-ClickHouse-Key": "warehouse-test-only"})
    try:
        with urllib.request.urlopen(request, timeout=90) as response:
            return [json.loads(line) for line in response.read().decode().splitlines() if line]
    except urllib.error.HTTPError as error:
        raise AssertionError(error.read().decode()[:2000]) from error


def environment(network, url, work):
    env = os.environ.copy()
    env.update({"CLICKHOUSE_URL": url, "CLICKHOUSE_USER": "warehouse_test",
                "CLICKHOUSE_PASSWORD": "warehouse-test-only", "CLICKHOUSE_DB_TRON": DATABASES[network],
                "CLICKHOUSE_DB_ETH": DATABASES[network], "ETH_RPC_URL": "http://127.0.0.1:1",
                "ETH_EXPECTED_CHAIN_ID": "1", "ETH_NETWORK_ID": "eip155:1",
                "BSC_CLICKHOUSE_URL": url, "BSC_CLICKHOUSE_USER": "warehouse_test",
                "BSC_CLICKHOUSE_PASSWORD": "warehouse-test-only", "BSC_CLICKHOUSE_DATABASE": DATABASES[network],
                "BITCOIN_CLICKHOUSE_URL": url, "BITCOIN_CLICKHOUSE_USER": "warehouse_test",
                "BITCOIN_CLICKHOUSE_PASSWORD": "warehouse-test-only", "BITCOIN_CLICKHOUSE_DATABASE": DATABASES[network],
                "BITCOIN_WRITER_LOCK_DIR": str(Path(work) / "locks"), "TRON_WRITER_LOCK_DIR": str(Path(work) / "locks")})
    return env


def schema(url, database):
    objects = query(url, f"SELECT name,engine,engine_full,partition_key,sorting_key,primary_key,create_table_query,as_select FROM system.tables WHERE database='{database}' ORDER BY name")
    columns = query(url, f"SELECT table,name,type,position,default_kind,default_expression FROM system.columns WHERE database='{database}' ORDER BY table,position")
    indexes = query(url, f"SELECT table,name,type_full,expr,granularity FROM system.data_skipping_indices WHERE database='{database}' ORDER BY table,name")
    def normalize(value):
        if isinstance(value, str):
            return value.replace(f"`{database}`.", "`database`.").replace(f"{database}.", "database.")
        if isinstance(value, dict):
            return {key: normalize(item) for key, item in value.items()}
        if isinstance(value, list):
            return [normalize(item) for item in value]
        return value
    return normalize({"objects": objects, "columns": columns, "indexes": indexes})


def ledger(url, database):
    return [query(url, f"SELECT * FROM {database}.{table} FINAL ORDER BY network_id" +
                  (",schema_version" if table == "aml_schema_migrations" else ""))
            for table in ("aml_schema_migrations", "aml_warehouse_identity")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", action="append", required=True)
    parser.add_argument("--url", action="append", required=True)
    args = parser.parse_args()
    binaries = dict(value.split("=", 1) for value in args.binary)
    urls = dict(value.split("=", 1) for value in args.url)
    if set(binaries) != set(DATABASES) or set(urls) != set(DATABASES):
        parser.error("supply exactly tron, ethereum, bsc and bitcoin binaries and URLs")
    if len(set(urls.values())) != 4:
        parser.error("four independent test server endpoints are required")

    class Installation(unittest.TestCase):
        maxDiff = 1500

        def test_product_initializers_install_one_contract_on_four_empty_servers(self):
            contract = json.loads((ROOT / "docs/warehouse/current-contract.json").read_text(encoding="utf-8"))
            expected = {obj["name"] for obj in contract["tables"] + contract["views"]}
            shapes = []
            identities = []
            # Check every destination before invoking any mutating CLI.
            for network, database in DATABASES.items():
                self.assertEqual(int(query(urls[network], f"SELECT count() AS n FROM system.tables WHERE database='{database}'")[0]["n"]), 0,
                                 "requires fresh disposable databases; refusing existing tables")
                self.assertTrue(Path(binaries[network]).is_file())
            with tempfile.TemporaryDirectory(prefix="aml-schema-test-") as work:
                for network, database in DATABASES.items():
                    binary = str(Path(binaries[network]).resolve())
                    env = environment(network, urls[network], work)
                    for index, options in enumerate(([], ["--check"], [])):
                        result = subprocess.run([binary, *options], cwd=work, env=env, capture_output=True, text=True, timeout=300)
                        self.assertEqual(result.returncode, 0, f"{network} {options}: {result.stdout}\n{result.stderr}")
                        actual = schema(urls[network], database)
                        state = ledger(urls[network], database)
                        if index == 0:
                            first, first_state = actual, state
                        else:
                            self.assertEqual(actual, first, f"{network} schema changed on check/repeat init")
                            self.assertEqual(state, first_state, f"{network} ledger changed on check/repeat init")
                    self.assertEqual({obj["name"] for obj in first["objects"]}, expected, f"{network} has legacy or missing objects")
                    self.assertEqual(len(first_state[0]), contract["version"])
                    identity = first_state[1]
                    self.assertEqual(len(identity), 1)
                    self.assertEqual(identity[0]["network_id"], NETWORKS[network])
                    self.assertEqual(identity[0]["schema_version"], contract["version"])
                    identities.append(identity[0]["checksum"])
                    shapes.append(first)
                    physical = {obj["name"] for obj in first["objects"] if obj["engine"] != "View"}
                    print(f"{network}: {len(physical)} tables, {len(expected)-len(physical)} views, "
                          f"{sum(c['table'] in physical for c in first['columns'])} physical columns; init/check/repeat OK", flush=True)
            self.assertTrue(all(item == shapes[0] for item in shapes[1:]), "engines, keys, views, columns or indexes differ across networks")
            self.assertEqual(len(set(identities)), 1, "contract checksums differ across networks")

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Installation)
    assert suite.countTestCases() == 1, "schema acceptance was not discovered"
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(0 if result.wasSuccessful() else 1)


if __name__ == "__main__":
    main()
