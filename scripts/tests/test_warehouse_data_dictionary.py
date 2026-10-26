import copy
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("warehouse_data_dictionary", SCRIPTS / "warehouse_data_dictionary.py")
dictionary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dictionary)


class DataDictionaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.snapshot = dictionary.read_json(dictionary.SNAPSHOT)
        cls.report = dictionary.build(cls.snapshot)
        cls.targets = {t["name"]: t for t in dictionary.read_json(dictionary.CONTRACT)["tables"]}

    def test_every_materialized_object_and_column_has_an_explicit_disposition(self):
        for chain, source in self.snapshot["chains"].items():
            actual = {o["name"]: o for o in self.report["chains"][chain]["objects"]}
            self.assertEqual(set(actual), {o["name"] for o in source["objects"]})
            for obj in source["objects"]:
                columns = actual[obj["name"]]["columns"]
                self.assertEqual([c["source"] for c in columns], [c["name"] for c in obj["columns"]])
                for column in columns:
                    self.assertTrue(column["unit"] and column["rule"] and column["disposition"])
                    self.assertNotEqual(column["unit"], "requires_unit_decision", (chain, obj["name"], column["source"]))
                    if column["target"]:
                        table, field = column["target"].split(".")
                        self.assertIn(field, {c["name"] for c in self.targets[table]["columns"]})
                    else:
                        self.assertEqual(column["disposition"], "retain_legacy_until_replacement")

    def test_new_unreviewed_table_cannot_be_silently_ignored(self):
        with self.assertRaisesRegex(ValueError, "Unreviewed legacy object"):
            dictionary.object_mapping("new_evidence_table")

    def test_missing_or_duplicate_snapshot_object_cannot_hide_existing_ddl(self):
        for mutation in ("missing", "duplicate"):
            changed = copy.deepcopy(self.snapshot)
            objects = changed["chains"]["tron"]["objects"]
            if mutation == "missing":
                objects.pop()
            else:
                objects.append(copy.deepcopy(objects[0]))
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, "Legacy object coverage"):
                dictionary.validate_snapshot(changed)

    def test_shared_fixture_drift_in_any_network_is_rejected(self):
        changed = copy.deepcopy(self.snapshot)
        changed["chains"]["bsc"]["shared_objects"][0]["columns"][0]["type"] = "Float64"
        with self.assertRaisesRegex(ValueError, "Shared fixture shape"):
            dictionary.validate_snapshot(changed)

    def test_all_four_fixture_database_scopes_are_required(self):
        for mutation in ("missing", "wrong_database"):
            changed = copy.deepcopy(self.snapshot)
            if mutation == "missing":
                del changed["chains"]["bitcoin"]
            else:
                changed["chains"]["bitcoin"]["database"] = "bitcoin_aml"
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                dictionary.validate_snapshot(changed)

    def test_missing_ddl_file_and_changed_ddl_require_a_new_disposable_snapshot(self):
        changed = copy.deepcopy(self.snapshot)
        changed["chains"]["bsc"]["ddl_manifest"].pop()
        with self.assertRaisesRegex(ValueError, "DDL file coverage changed"):
            dictionary.validate_snapshot(changed)
        changed = copy.deepcopy(self.snapshot)
        changed["source_manifest"][0]["normalized_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "changed since materialization"):
            dictionary.validate_snapshot(changed)

    def test_bitcoin_total_and_time_do_not_acquire_false_transfer_semantics(self):
        objects = {o["name"]: o for o in self.report["chains"]["bitcoin"]["objects"]}
        columns = {c["source"]: c for c in objects["bitcoin_transactions"]["columns"]}
        self.assertEqual(columns["output_sats"]["target"], "aml_transactions.raw_evidence_json")
        self.assertIn("amount_raw remains NULL", columns["output_sats"]["rule"])
        self.assertEqual(columns["block_time"]["unit"], "unix_seconds")
        self.assertIn("multiply by 1000", columns["block_time"]["rule"])
        self.assertIn("do not copy", columns["commit_id"]["rule"])
        marker = {c["source"]: c for c in objects["bitcoin_block_commits"]["columns"]}
        self.assertIsNone(marker["revision"]["target"])

    def test_legacy_ledgers_are_not_rewritten_as_shared_migration_history(self):
        for chain in ("ethereum", "bsc"):
            ledger = next(o for o in self.report["chains"][chain]["objects"] if o["name"] == "schema_migrations")
            self.assertEqual(ledger["mapping"]["strategy"], "retain_legacy_ledger")
            self.assertIsNone(ledger["mapping"]["target_table"])
            self.assertEqual(len(ledger["columns"]), 4)

    def test_generic_bindings_and_exported_sql_are_distinguished_from_live_writes(self):
        objects = {o["name"]: o for o in self.report["chains"]["tron"]["objects"]}
        transactions = objects["transactions"]
        self.assertTrue(any(r["role"] == "writer_binding_requires_generic_dispatch" for r in transactions["access_evidence"]))
        self.assertTrue(any("generic writer dispatch" == r["role"] for r in transactions["call_chain_evidence"]))
        registry = objects["wallet_ml_model_registry"]
        self.assertTrue(any(r["role"] == "sql_export_producer" for r in registry["access_evidence"]))
        self.assertFalse(any(r["role"] in ("literal_writer", "insert_call_writer") for r in registry["access_evidence"]))

    def test_test_only_import_does_not_hide_the_active_bsc_ingestion_writer(self):
        transactions = next(o for o in self.report["chains"]["bsc"]["objects"] if o["name"] == "transactions")
        self.assertTrue(any(r["path"].endswith("ingestion/store.rs") and r["role"] == "insert_call_writer"
                            for r in transactions["access_evidence"]))

    def test_materialized_views_include_ddl_triggered_writes_and_dependencies(self):
        objects = {o["name"]: o for o in self.report["chains"]["tron"]["objects"]}
        for name in ("mv_wallet_asset_delta_transfer_from_v3", "mv_wallet_asset_delta_transfer_to_v3"):
            self.assertEqual(objects[name]["materialized_target"], "wallet_asset_balance_deltas_v3")
            self.assertIn("address_relationships", objects[name]["dependencies"])
        self.assertEqual(set(objects["wallet_asset_balance_deltas_v3"]["access_summary"]["materialized_writers"]),
                         {"mv_wallet_asset_delta_transfer_from_v3", "mv_wallet_asset_delta_transfer_to_v3"})

    def test_seed_only_policy_and_review_helper_have_different_writer_evidence(self):
        objects = {o["name"]: o for o in self.report["chains"]["ethereum"]["objects"]}
        policy = objects["risk_policies"]["access_summary"]
        self.assertTrue(policy["seed_writers"])
        self.assertFalse(policy["literal_or_helper_writers"])
        self.assertTrue(objects["intelligence_reviews"]["access_summary"]["literal_or_helper_writers"])
        self.assertFalse(objects["ingestion_benchmarks"]["access_summary"]["literal_or_helper_writers"])

    def test_incompatible_existing_sql_export_is_reported_without_being_executed(self):
        conflicts = {c["table"]: c for c in self.report["source_schema_conflicts"]}
        self.assertEqual(conflicts["wallet_ml_training_runs"]["status"], "table_absent_from_current_DDL")
        self.assertIn("training_run_id", conflicts["wallet_ml_model_registry"]["missing_columns"])
        self.assertIn("deployment_id", conflicts["wallet_ml_model_deployments"]["missing_columns"])

    def test_required_publication_inputs_cannot_disappear_behind_json_mapping(self):
        obj = next(o for o in self.report["chains"]["tron"]["objects"] if o["name"] == "transactions")
        required = {c["column"]: c["recipe"] for c in obj["required_target_inputs"]}
        for field in ("network_id", "block_hash", "commit_id", "revision", "transaction_index"):
            self.assertIn(field, required)
        self.assertIn("never constant zero", required["transaction_index"])

    def test_stale_generic_owner_anchor_fails_instead_of_claiming_verified_ownership(self):
        changed = {"bitcoin": [("core", "src/storage/writer.rs", "missing_dispatch_symbol", "writer")]}
        with patch.dict(dictionary.OWNERS, changed), self.assertRaisesRegex(ValueError, "Stale owner evidence"):
            dictionary.owner_evidence("bitcoin")

    def test_empty_shared_tables_are_not_reported_as_working_bitcoin_analytics(self):
        self.assertIn("installed only", dictionary.shared_runtime("bitcoin", "aml_wallet_risk_assessments"))
        self.assertIn("not_applicable", dictionary.shared_runtime("bitcoin", "aml_evm_logs"))
        self.assertIn("cutover remains open", dictionary.shared_runtime("ethereum", "aml_transactions"))

    def test_shared_numeric_columns_have_explicit_units_too(self):
        for obj in self.report["shared_objects"]:
            for column in obj["columns"]:
                self.assertNotEqual(column["unit"], "requires_unit_decision", (obj["name"], column["name"]))


if __name__ == "__main__":
    unittest.main()
