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

    def test_current_core_target_does_not_relabel_the_historical_baseline(self):
        self.assertEqual(self.report["schema_version"],1)
        self.assertEqual(self.report["counts"]["common"]["table_columns"],376)
        current=self.report["target_core_contract"]
        self.assertEqual((current["version"],current["tables"],current["views"],current["table_columns"]),(3,29,21,404))
        self.assertIn("DB-08",current["scope"])

    def test_stale_current_core_columns_are_rejected(self):
        read=dictionary.read_json
        def changed(path):
            data=read(path)
            if path.name == "core-contract-v3.json":
                data["tables"][0]["columns"].pop()
            return data
        with patch.object(dictionary,"read_json",side_effect=changed),self.assertRaisesRegex(ValueError,"column export is stale"):
            dictionary.build(self.snapshot)
    def test_analytics_target_preserves_baseline_and_records_nine_additive_tables(self):
        target=self.report["target_analytics_contract"]
        self.assertEqual((target["version"],target["tables"],target["views"],target["table_columns"]),(4,38,40,668))
        self.assertEqual(len(target["new_tables"]),9)
        self.assertEqual(self.report["schema_version"],1)
    def test_analytics_plan_keeps_pair_claim_endpoints_and_original_types(self):
        mapping=next(m for m in self.report["target_analytics_mapping"] if m["chain"]=="ethereum" and m["source_object"]=="address_cluster_claims")
        fields={c["source"]:c for c in mapping["columns"]}
        self.assertEqual(fields["right_address"]["candidate"],"aml_cluster_claims.right_subject_id")
        self.assertTrue(all(c["source_type"] and c["original_evidence_path"] for c in fields.values()))
        self.assertIn("no backfill",mapping["status"])
    def test_changed_analytics_view_cannot_pass_as_the_current_export(self):
        original=dictionary.read_json
        def changed(path):
            data=original(path)
            if path.name=="analytics-contract-v4.json":data["views"][-1]["select"]="SELECT 1"
            return data
        with patch.object(dictionary,"read_json",side_effect=changed),self.assertRaisesRegex(ValueError,"view export is stale"):
            dictionary.build(self.snapshot)
    def test_relationship_contract_does_not_claim_a_storage_migration_or_legacy_cutover(self):
        contract=self.report["relationship_interpretation"]
        self.assertEqual(contract["version"],"typed_relationships_v1")
        self.assertEqual(contract["storage_schema_version"],5)
        self.assertEqual(contract["introduced_storage_schema_version"],4)
        self.assertFalse(contract["mutates_storage"])
        self.assertIn("DB-08",contract["other_networks"])
    def test_visibility_contract_preserves_history_and_exports_current_target(self):
        target=self.report["target_visibility_contract"]
        self.assertEqual((target["version"],target["tables"],target["views"],target["table_columns"]),(5,39,53,682))
        self.assertIn("DB-08",target["scope"])
    def test_backup_proof_is_separate_from_schema_migration_and_cutover(self):
        recovery = self.report["migration_recovery"]
        self.assertEqual(recovery["stage"], "DB-08A")
        self.assertEqual(recovery["storage_schema_version"], 5)
        self.assertFalse(recovery["schema_changed"])
        self.assertFalse(recovery["cutover_allowed"])
        for source in recovery["source_manifest"]:
            self.assertEqual(source["normalized_sha256"], dictionary.normalized_hash(dictionary.ROOT / source["path"]))
        self.assertEqual(len(recovery["remaining"]), 3)
        acceptance = dictionary.read_json(dictionary.ROOT / recovery["acceptance"])
        self.assertEqual({n["network"] for n in acceptance["networks"]}, set(dictionary.NETWORKS))
        self.assertFalse(acceptance["shared_schema_changed"])
        self.assertTrue(all(n["restore_verified"] and not n["cutover_allowed"] for n in acceptance["networks"]))
    def test_changed_visibility_view_export_is_rejected(self):
        original=dictionary.read_json
        def changed(path):
            data=original(path)
            if path.name=="current-contract.json":data["views"][-1]["select"]="SELECT 1"
            return data
        with patch.object(dictionary,"read_json",side_effect=changed),self.assertRaisesRegex(ValueError,"visibility view export is stale"):
            dictionary.build(self.snapshot)


if __name__ == "__main__":
    unittest.main()
