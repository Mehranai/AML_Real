import copy
import importlib.util
import io
import json
from pathlib import Path
import re
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("warehouse_backup", ROOT / "scripts/warehouse_backup.py")
backup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backup)


def source():
    objects = [{"name": "evidence", "engine": "MergeTree", "ddl": "CREATE TABLE {{database}}.evidence (v UInt256) ENGINE=MergeTree ORDER BY v", "data": {"rows": 1}}]
    return {"server": {"version": "23.8.16.16", "timezone": "UTC"}, "encoding": backup.ROW_ENCODING,
            "network": "bitcoin", "network_id": backup.NETWORKS["bitcoin"],
            "identity": [], "network_binding": "shared_identity_verified", "objects": objects, "sha256": backup.digest(objects)}


class FakeClient:
    url = "http://localhost:8123"
    def __init__(self):
        self.calls, self.jobs = [], {}
        self.existing = False
        self.uncertain = False
        self.failed = False
        self.version = source()["server"]
    def rows(self, sql, **kwargs):
        self.calls.append(sql)
        if "system.disks" in sql:
            return [{"name": backup.DISK}]
        if "system.backups" in sql:
            operation_id = re.search(r"WHERE id='([^']+)'", sql)[1]
            return [self.jobs[operation_id]] if operation_id in self.jobs else []
        if "version()" in sql:
            return [self.version]
        if "system.databases" in sql:
            return [{"name": "target"}] if self.existing else []
        raise AssertionError(sql)
    def execute(self, sql):
        self.calls.append(sql)
        operation_id = re.search(r"id='([^']+)'", sql)[1]
        status = "BACKUP_CREATED" if sql.startswith("BACKUP") else "RESTORED"
        self.jobs[operation_id] = {"id": operation_id, "name": "fixture", "status": "BACKUP_FAILED" if self.failed else status, "error": "private server text"}
        if self.uncertain:
            raise OSError("uncertain response")


class BackupTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "backup.json"
        self.restore_path = Path(self.directory.name) / "restore.json"
        self.client = FakeClient()
        self.snapshot = patch.object(backup, "stable_snapshot", side_effect=lambda *_: source()).start()
        self.parts = patch.object(backup, "parts_signature", return_value="stable").start()
        self.prepare = patch.object(backup, "prepare_restore_data").start()
        self.addCleanup(patch.stopall)
    def create(self):
        return backup.backup(self.client, "source", "bitcoin", self.path, paused=True)
    def test_identifiers_and_origins_are_validated(self):
        for name in ("a; DROP DATABASE x", "a.b", "../file", "0bad", "", "`name`"):
            with self.assertRaises(ValueError): backup.identifier(name)
        for url in ("file:///tmp/x", "http://u:p@localhost", "http://localhost/?secret=x", "http://localhost/db", "http://localhost/#x"):
            with self.assertRaises(ValueError): backup.origin(url)
        self.assertEqual(backup.origin("https://localhost:8123/"), "https://localhost:8123")
    def test_paused_acknowledgement_precedes_io(self):
        with self.assertRaises(ValueError): backup.backup(self.client, "source", "bitcoin", self.path, paused=False)
        self.assertEqual(self.client.calls, [])
    def test_native_backup_is_async_and_has_a_persisted_id(self):
        result = self.create()
        self.assertEqual(result["status"], "verified")
        query = next(q for q in self.client.calls if q.startswith("BACKUP"))
        self.assertIn(result["operation_id"], query)
        self.assertTrue(query.endswith("ASYNC"))
        self.assertNotIn("FORMAT", query)  # BACKUP FORMAT is rejected by pinned 23.8.
        self.assertEqual(backup.load(self.path)["status"], "verified")
    def test_manifest_cannot_be_overwritten_implicitly(self):
        self.create()
        with self.assertRaises(ValueError): self.create()
    def test_uncertain_dispatch_is_reconciled_without_a_second_backup(self):
        self.client.uncertain = True
        self.assertEqual(self.create()["status"], "dispatched")
        self.client.uncertain = False
        result = backup.backup(self.client, "source", "bitcoin", self.path, paused=True, resume=True)
        self.assertEqual(result["status"], "verified")
        self.assertEqual(sum(q.startswith("BACKUP") for q in self.client.calls), 1)
    def test_missing_server_operation_is_not_resubmitted(self):
        self.client.uncertain = True; self.create(); self.client.jobs.clear()
        with self.assertRaises(ValueError): backup.backup(self.client, "source", "bitcoin", self.path, paused=True, resume=True)
        self.assertEqual(sum(q.startswith("BACKUP") for q in self.client.calls), 1)
    def test_failed_server_operation_is_not_verified_or_disclosed(self):
        self.client.failed = True
        result = self.create()
        self.assertEqual(result["status"], "rejected")
        self.assertNotIn("private server text", json.dumps(backup.public_status(result)))
    def test_changed_source_is_rejected(self):
        other = source(); other["objects"][0]["data"]["rows"] = 2; other["sha256"] = backup.digest(other["objects"])
        self.snapshot.side_effect = [source(), other]
        self.assertEqual(self.create()["status"], "rejected")
    def test_same_content_with_changed_parts_is_rejected(self):
        self.parts.side_effect = ["stable", "stable", "stable", "new-part"]
        self.assertEqual(self.create()["status"], "rejected")
    def test_prepared_resume_refuses_changed_parts_before_dispatch(self):
        result = self.create(); result["status"] = "prepared"; self.client.jobs.clear(); backup.save(self.path, result)
        self.parts.return_value = "new-part"
        count = len(self.client.calls)
        result = backup.backup(self.client, "source", "bitcoin", self.path, paused=True, resume=True)
        self.assertEqual(result["status"], "rejected")
        self.assertFalse(any(q.startswith("BACKUP") for q in self.client.calls[count:]))
    def test_restore_only_targets_an_absent_database(self):
        self.create(); self.client.existing = True
        with self.assertRaises(ValueError): backup.restore(self.client, self.path, "target", self.restore_path)
        self.assertFalse(self.restore_path.exists())
        self.assertFalse(any(q.startswith("RESTORE") for q in self.client.calls))
    def test_restore_verifies_all_source_content(self):
        self.create()
        result = backup.restore(self.client, self.path, "target", self.restore_path)
        self.assertEqual(result["status"], "verified")
        query = next(q for q in self.client.calls if q.startswith("RESTORE") and "structure_only" not in q)
        self.assertIn("allow_non_empty_tables=false", query)
        self.assertIn("allow_different_table_def=true", query)
        proof = backup.compare_receipts(self.path, self.restore_path)
        self.assertTrue(proof["restore_verified"])
        self.assertFalse(proof["cutover_allowed"])
        self.assertEqual(result["phase"], "data")
        self.prepare.assert_called_once()
    def test_uncertain_data_phase_resumes_without_reinserting(self):
        self.create(); original = self.client.execute
        def execute(sql):
            original(sql)
            if sql.startswith("RESTORE") and "structure_only" not in sql:
                raise OSError("response lost after data job started")
        with patch.object(self.client, "execute", side_effect=execute):
            result = backup.restore(self.client, self.path, "target", self.restore_path)
        self.assertEqual((result["phase"], result["status"]), ("data", "dispatched"))
        result = backup.restore(self.client, self.path, "target", self.restore_path, resume=True)
        self.assertEqual(result["status"], "verified")
        self.assertEqual(sum(q.startswith("RESTORE") and "structure_only" not in q for q in self.client.calls), 1)
    def test_uncertain_structure_phase_resumes_without_recreating(self):
        self.create(); self.client.uncertain = True
        result = backup.restore(self.client, self.path, "target", self.restore_path)
        self.assertEqual(result["phase"], "structure")
        self.client.uncertain = False
        result = backup.restore(self.client, self.path, "target", self.restore_path, resume=True)
        self.assertEqual(result["status"], "verified")
        self.assertEqual(sum(q.startswith("RESTORE") and "structure_only" in q for q in self.client.calls), 1)
    def test_release_checks_content_and_only_resumes_the_verified_target(self):
        self.create(); backup.restore(self.client, self.path, "target", self.restore_path)
        with patch.object(self.client, "execute") as command:
            result = backup.release_restored_merges(self.client, self.restore_path)
            self.assertEqual(result["merges"], "resumed")
            command.assert_called_once_with("SYSTEM START MERGES `target`.`evidence`")
    def test_changed_restore_is_not_released(self):
        self.create(); backup.restore(self.client, self.path, "target", self.restore_path)
        self.snapshot.side_effect = lambda *_: {"different": True}
        with patch.object(self.client, "execute") as command, self.assertRaises(ValueError):
            backup.release_restored_merges(self.client, self.restore_path)
        command.assert_not_called()
    def test_restore_mismatch_is_rejected(self):
        self.create(); other = source(); other["sha256"] = "changed"
        self.snapshot.side_effect = lambda *_: other
        self.assertEqual(backup.restore(self.client, self.path, "target", self.restore_path)["status"], "rejected")
    def test_restore_requires_a_verified_backup(self):
        self.client.failed = True; self.create()
        with self.assertRaises(ValueError): backup.restore(self.client, self.path, "target", self.restore_path)
    def test_version_and_timezone_mismatch_precede_restore(self):
        self.create(); self.client.version = {"version": "24.1", "timezone": "UTC"}
        with self.assertRaises(ValueError): backup.restore(self.client, self.path, "target", self.restore_path)
    def test_manifest_identity_path_and_digest_tampering_fail_before_io(self):
        result = self.create()
        for field, value in (("archive", "../other.zip"), ("database", "x`; DROP DATABASE y"), ("operation_id", "bad"), ("network", "bsc"), ("verified_sha256", "bad")):
            changed = copy.deepcopy(result); changed[field] = value; backup.save(self.path, changed)
            with self.subTest(field=field), self.assertRaises(ValueError): backup.load(self.path)
        changed = copy.deepcopy(result); changed["source"]["objects"][0]["data"]["rows"] = 99; backup.save(self.path, changed)
        with self.assertRaises(ValueError): backup.load(self.path)
    def test_receipts_bind_the_exact_backup_attempt(self):
        result = self.create(); backup.restore(self.client, self.path, "target", self.restore_path)
        result["source_parts_sha256"] = "different"; backup.save(self.path, result)
        with self.assertRaises(ValueError): backup.compare_receipts(self.path, self.restore_path)
    def test_resume_must_match_the_target_and_endpoint(self):
        self.create(); backup.restore(self.client, self.path, "target", self.restore_path)
        with self.assertRaises(ValueError): backup.restore(self.client, self.path, "other", self.restore_path, resume=True)
        self.client.url = "http://other:8123"
        with self.assertRaises(ValueError): backup.refresh(self.client, self.restore_path)
    def test_public_status_excludes_internal_ddl_and_endpoints(self):
        result = self.create()
        text = json.dumps(backup.public_status(result))
        self.assertNotIn("localhost", text); self.assertNotIn("CREATE TABLE", text)


class EncodingTests(unittest.TestCase):
    def test_template_render_preserves_literal_placeholders(self):
        sql = "CREATE VIEW {{database}}.v AS SELECT '{{database}}.literal' FROM {{database}}.data"
        self.assertEqual(backup.render_sql(sql, "target"), "CREATE VIEW `target`.v AS SELECT '{{database}}.literal' FROM `target`.data")
    def test_view_order_is_topological_and_cycles_fail(self):
        def view(name, needed): return {"name": name, "engine": "View", "ddl": f"CREATE VIEW {{{{database}}}}.{name} AS SELECT * FROM {{{{database}}}}.{needed}"}
        self.assertEqual([v["name"] for v in backup.ordered_views([view("a", "z"), view("z", "raw")])], ["z", "a"])
        with self.assertRaises(ValueError): backup.ordered_views([view("a", "z"), view("z", "a")])
    def test_alias_repair_does_not_change_physical_tables(self):
        physical = {"name": "evidence", "engine": "MergeTree", "ddl": "CREATE TABLE {{database}}.evidence (v UInt256) ENGINE=MergeTree ORDER BY v", "columns": []}
        view = {"name": "v", "engine": "View", "ddl": "CREATE VIEW {{database}}.v AS SELECT f.v FROM {{database}}.evidence AS f", "columns": []}
        broken = copy.deepcopy(view); broken["ddl"] = broken["ddl"].replace(" AS f", "")
        class SchemaClient:
            def __init__(self): self.commands = []
            def rows(self, *_): return [{"n": "0"}]
            def execute(self, sql, **kwargs): self.commands.append((sql, kwargs))
        client = SchemaClient()
        with patch.object(backup, "snapshot", side_effect=[{"objects": [physical, broken]}, {"objects": [physical, view]}]):
            backup.prepare_restore_data(client, {"database": "target", "network": "bitcoin", "source": {"objects": [physical, view]}})
        self.assertEqual(client.commands[0][0], "SYSTEM STOP MERGES `target`.`evidence`")
        self.assertIn("AS f", client.commands[1][0])
        self.assertFalse(any("DROP TABLE" in sql or "CREATE TABLE" in sql for sql, _ in client.commands))
    def test_nonempty_structure_cannot_enter_data_restore(self):
        table = {"name": "evidence", "engine": "MergeTree", "ddl": "ddl", "columns": []}
        client = type("SchemaClient", (), {"rows": lambda *_: [{"n": "2"}], "execute": lambda *_: self.fail("unexpected mutation")})()
        with patch.object(backup, "snapshot", return_value={"objects": [table]}), self.assertRaises(ValueError):
            backup.prepare_restore_data(client, {"database": "target", "network": "bitcoin", "source": {"objects": [table]}})
    def test_every_repository_column_type_has_an_explicit_encoder(self):
        report = json.loads((ROOT / "docs/warehouse/data-dictionary.json").read_text(encoding="utf-8"))
        kinds = {c["type"] for network in report["chains"].values() for obj in network["objects"] for c in obj["columns"]}
        contract = json.loads((ROOT / "docs/warehouse/current-contract.json").read_text(encoding="utf-8"))
        kinds.update(c["data_type"] for t in contract["tables"] for c in t["columns"])
        for kind in kinds:
            with self.subTest(kind=kind): self.assertTrue(backup.value_expression("`v`", kind))
    def test_unknown_state_types_are_rejected(self):
        for kind in ("AggregateFunction(sum, UInt256)", "Object('json')", "Dynamic", "Map(String, String)"):
            with self.assertRaises(ValueError): backup.value_expression("`v`", kind)
    def test_binary_strings_and_float_bits_are_preserved(self):
        self.assertEqual(backup.value_expression("v", "String"), "hex(v)")
        self.assertIn("reinterpretAsString", backup.value_expression("v", "Float64"))
        self.assertIn("arrayMap", backup.value_expression("v", "Array(String)"))
        self.assertIn("isNull", backup.value_expression("v", "Nullable(UInt64)"))
    def test_sql_normalization_does_not_rewrite_literals(self):
        value = "SELECT source.x,`source`.y,'source.z', 'it\\'s source.a' FROM source.t"
        self.assertEqual(backup.normalize_sql(value, "source"), "SELECT {{database}}.x,{{database}}.y,'source.z', 'it\\'s source.a' FROM {{database}}.t")
    def test_unbounded_integer_totals_do_not_wrap_or_turn_null_into_zero(self):
        maximum = 2**256 - 1
        rows = [[str(maximum), "1"], [str(maximum), None]]
        body = b"".join((json.dumps({"row": json.dumps(row)}) + "\n").encode() for row in rows)
        client = type("Stream", (), {"open": lambda *_a, **_k: io.BytesIO(body)})()
        result = backup.table_multiset(client, "source", "table", [{"name": "value", "type": "UInt256"}, {"name": "value_sats", "type": "Nullable(UInt64)"}])
        self.assertEqual(result["rows"], 2)
        self.assertEqual(result["exact_amount_totals"][0]["total"], str(maximum * 2))
        self.assertEqual(result["exact_amount_totals"][1], {"column": "value_sats", "known_rows": 1, "null_rows": 1, "total": "1"})


if __name__ == "__main__":
    unittest.main()
