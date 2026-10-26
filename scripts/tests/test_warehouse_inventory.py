import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    "warehouse_inventory", Path(__file__).resolve().parents[1] / "warehouse_inventory.py"
)
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class InventoryTests(unittest.TestCase):
    def test_sql_literals_and_comments_do_not_split_statements(self):
        sql = """-- ; comment
CREATE TABLE `semi;colon` (v String DEFAULT 'a; b'';c');
/* ; ignored */ INSERT INTO `semi;colon` VALUES ('escaped\\';value');
SELECT 1 -- final ; comment
"""
        self.assertEqual(inventory.statements(sql), [
            "CREATE TABLE `semi;colon` (v String DEFAULT 'a; b'';c')",
            "INSERT INTO `semi;colon` VALUES ('escaped\\';value')",
            "SELECT 1",
        ])

    def test_incomplete_sql_fails_before_execution(self):
        for sql in ("SELECT 'unfinished", "SELECT `unfinished", "SELECT 1 /* unfinished"):
            with self.subTest(sql=sql), self.assertRaises(ValueError):
                inventory.statements(sql)

    def test_only_local_test_origins_are_accepted(self):
        for url in ("http://127.0.0.1:48932", "http://localhost:8123/"):
            inventory.TestClickHouse(url, "test", "test")
        for url in ("http://production:8123", "file:///tmp/db", "http://localhost/live",
                    "http://test:secret@localhost", "http://localhost/?database=live",
                    "http://localhost/#live", "http://127.0.0.1.evil.example"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                inventory.TestClickHouse(url, "test", "test")

    def test_database_scope_is_checked_before_http(self):
        client = inventory.TestClickHouse("http://localhost:1", "test", "test")
        for database in ("production", "aml_inventory_tron; SELECT 1", "aml_inventory_"):
            with self.subTest(database=database), self.assertRaises(ValueError):
                client.execute("SELECT 1", database=database)


if __name__ == "__main__":
    unittest.main()
