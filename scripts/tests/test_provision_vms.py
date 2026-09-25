"""Unit and simulated CLI integration tests; never create real VMs."""
import argparse
import contextlib
import copy
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("provision_vms", Path(__file__).parents[1] / "provision_vms.py")
vm = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(vm)


class FakeMultipass:
    def __init__(self):
        self.calls, self.instances, self.transfers, self.envs = [], {}, {}, {}
        self.fail_up = None

    def __call__(self, args, **kwargs):
        args = [str(value) for value in args]
        self.calls.append(args)
        if args[0] != "multipass":
            raise AssertionError(f"Unexpected host command: {args}")
        action = args[1]
        if action == "list":
            return json.dumps({"list": [{"name": name, "state": item["state"], "ipv4": [item["ip"]]}
                                       for name, item in self.instances.items()]})
        if action == "launch":
            name = args[args.index("--name") + 1]
            if name in self.instances:
                raise AssertionError("Duplicate launch")
            cloud = json.loads(Path(args[args.index("--cloud-init") + 1]).read_text().split("\n", 1)[1])
            owner = json.loads(cloud["write_files"][0]["content"])
            self.instances[name] = {"state": "Running", "owner": owner,
                                    "ip": f"192.168.64.{10 + vm.ROLES.index(owner['role'])}"}
        elif action in ("start", "stop"):
            self.instances[args[2]]["state"] = "Running" if action == "start" else "Stopped"
        elif action == "transfer":
            name, remote = args[-1].split(":", 1)
            self.transfers[name, remote] = Path(args[2])
        elif action == "exec":
            name = args[2]
            command = args[args.index("--") + 1:]
            if command[:2] == ["cat", "/etc/aml-vm-owner.json"]:
                return json.dumps(self.instances[name]["owner"])
            if command[:3] == ["ip", "-j", "route"]:
                return json.dumps([{"prefsrc": self.instances[name]["ip"], "dev": "eth0"}])
            if command[0] == "sha256sum":
                return vm.sha256(self.transfers[name, command[1]]) + "  archive\n"
            if command[:4] == ["sudo", "docker", "image", "inspect"]:
                return json.dumps([{"Os": "linux", "Architecture": "amd64", "Id": "sha256:test"}])
            if command[:2] == ["tar", "-xzf"]:
                with tarfile.open(self.transfers[name, command[2]]) as archive:
                    for member in archive:
                        if member.name.endswith(".env"):
                            self.envs[name] = archive.extractfile(member).read().decode()
            if command[:2] == ["sudo", "bash"] and command[2].endswith("/scripts/vm.sh"):
                if command[4] == "up" and self.fail_up == command[3]:
                    self.fail_up = None
                    raise vm.DeploymentError("Injected deployment failure")
        else:
            raise AssertionError(f"Unexpected multipass action: {args}")
        return ""


class ProvisionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.config = {"prefix": "aml-test", "ubuntu": "24.04",
                       "roles": {role: {"cpus": 2, "memory_gib": 3, "disk_gib": 30} for role in vm.ROLES}}
        self.write("deployment/vms.json", json.dumps(self.config))
        self.write("deployment/vm-firewall.sh", "#!/bin/bash\r\ntrue\r\n")
        self.write("scripts/vm.sh", "#!/bin/bash\r\ntrue\r\n")
        self.write("gateway/auth/disabled.htpasswd", "")
        self.write("dockerizd_tron/app/sql/init_database_tron.sql", "SELECT 1;\r\n")
        for role, directory in vm.DIRECTORIES.items():
            password_key = ("NEO4J_PASSWORD" if role == "main" else
                            "BSC_CLICKHOUSE_PASSWORD" if role == "bsc" else "CLICKHOUSE_PASSWORD")
            self.write(f"{directory}/.env",
                       f"{password_key}=test-password\nAML_SERVICE_KEY=" + "x" * 40 + "\n")
            self.write(f"{directory}/" + ("compose.yaml" if role == "main" else "docker-compose.yml"),
                       "services: {}\r\n")
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        self.entries = []
        for index, image in enumerate(vm.ALL_IMAGES):
            path = self.bundle / f"{index}.tar"
            path.write_bytes(f"test image {index}".encode())
            self.entries.append({"image": image, "file": path.name, "sha256": vm.sha256(path)})
        self.manifest()
        self.args = argparse.Namespace(config=self.root / "deployment/vms.json", bundle=self.bundle,
                                       docker_context=None, build=False, storage_path=self.root,
                                       with_ingestion=False)
        self.runner = FakeMultipass()
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        self.stack.enter_context(patch.object(vm.shutil, "which", return_value="/bin/fake"))
        self.stack.enter_context(patch.object(vm.shutil, "disk_usage",
                                             return_value=shutil._ntuple_diskusage(1000 * vm.GIB, 0, 1000 * vm.GIB)))
        self.stack.enter_context(patch.object(vm, "available_memory", return_value=100 * vm.GIB))
        self.stack.enter_context(patch.object(vm.platform, "machine", return_value="AMD64"))

    def write(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content.encode())

    def manifest(self):
        (self.bundle / "manifest.json").write_text(json.dumps(self.entries))

    def app(self):
        return vm.Provisioner(self.args, self.root, self.runner)

    def launches(self):
        return [call for call in self.runner.calls if call[1] == "launch"]

    def test_config_rejects_missing_network_and_low_resources(self):
        for change in ("role", "size", "version", "name"):
            config = copy.deepcopy(self.config)
            if change == "role":
                del config["roles"]["bsc"]
            elif change == "size":
                config["roles"]["main"]["disk_gib"] = 1
            elif change == "version":
                config["ubuntu"] = "latest"
            else:
                config["prefix"] = "../unsafe"
            self.write("invalid.json", json.dumps(config))
            with self.assertRaises(vm.DeploymentError):
                vm.load_config(self.root / "invalid.json")

    def test_env_literals_and_duplicate_rejection(self):
        self.write("test.env", "# comment\r\nVALUE='001'\r\nEMPTY=\r\n")
        self.assertEqual(vm.read_env(self.root / "test.env"), {"VALUE": "001", "EMPTY": ""})
        for text in ("VALUE=1\nVALUE=2\n", "VALUE=${SECRET}\n", "bad-key=x\n"):
            self.write("test.env", text)
            with self.assertRaises(vm.DeploymentError):
                vm.read_env(self.root / "test.env")

    def test_short_existing_clickhouse_password_is_preserved(self):
        path = self.root / vm.DIRECTORIES["tron"] / ".env"
        path.write_text(path.read_text().replace("test-password", "short"))
        self.assertEqual(vm.source_environments(self.root)["tron"]["CLICKHOUSE_PASSWORD"], "short")

    def test_key_mismatch_fails_before_creation(self):
        self.write("dockerizd_bsc/.env", "BSC_CLICKHOUSE_PASSWORD=test\nAML_SERVICE_KEY=wrong\n")
        with self.assertRaisesRegex(vm.DeploymentError, "does not match"):
            self.app().up()
        self.assertFalse(self.launches())

    def test_bundle_must_include_all_seven_images(self):
        self.entries.pop()
        self.manifest()
        with self.assertRaisesRegex(vm.DeploymentError, "Incomplete"):
            vm.verify_bundle(self.bundle)

    def test_bundle_rejects_corruption_traversal_duplicates_and_nonobjects(self):
        original = copy.deepcopy(self.entries)
        for value in ("../escape.tar", self.entries[1]["file"]):
            self.entries = copy.deepcopy(original)
            self.entries[0]["file"] = value
            self.manifest()
            with self.assertRaises(vm.DeploymentError):
                vm.verify_bundle(self.bundle)
        self.entries = [None]
        self.manifest()
        with self.assertRaises(vm.DeploymentError):
            vm.verify_bundle(self.bundle)
        self.entries = original
        self.manifest()
        (self.bundle / "0.tar").write_bytes(b"tampered")
        with self.assertRaisesRegex(vm.DeploymentError, "checksum"):
            vm.verify_bundle(self.bundle)

    def test_runtime_is_allowlisted_and_has_linux_line_endings(self):
        self.write(".env.private-backup", "PRIVATE")
        self.write(".git/secret", "PRIVATE")
        self.write("dockerizd_tron/app/target/huge", "BUILD")
        path = self.root / "runtime.tar.gz"
        values = {"CLICKHOUSE_PASSWORD": "test"}
        vm.runtime_archive(self.root, path, "tron", values)
        with tarfile.open(path) as archive:
            self.assertEqual(len(archive.getnames()), 4)
            self.assertNotIn(b"\r", archive.extractfile("scripts/vm.sh").read())
            self.assertEqual(archive.getmember("dockerizd_tron/app/.env").mode, 0o600)
            self.assertNotIn(".env.private-backup", archive.getnames())

    def test_missing_multipass_and_resources_block_without_state_or_vm(self):
        for mode in ("tool", "ram", "disk"):
            mocked = (patch.object(vm.shutil, "which", return_value=None) if mode == "tool" else
                      patch.object(vm, "available_memory", return_value=vm.GIB) if mode == "ram" else
                      patch.object(vm.shutil, "disk_usage", return_value=shutil._ntuple_diskusage(vm.GIB, 0, vm.GIB)))
            with mocked, self.assertRaisesRegex(vm.DeploymentError, "Preflight"):
                self.app().up()
            self.assertFalse(self.launches())
            self.assertFalse((self.root / ".local-vms/state.json").exists())
            self.assertFalse((self.root / ".local-vms/operation.lock").exists())

    def test_doctor_has_no_writes(self):
        self.app().preflight()
        self.assertFalse((self.root / ".local-vms").exists())
        self.assertFalse(self.launches())

    def test_preflight_uses_full_vm_memory_not_fixed_two_gib(self):
        with patch.object(vm, "available_memory", return_value=6 * vm.GIB):
            with self.assertRaisesRegex(vm.DeploymentError, "Preflight"):
                self.app().preflight()
        self.assertFalse(self.launches())

    def test_preflight_checks_explicit_storage_without_relocating_it(self):
        with patch.object(vm.shutil, "disk_usage", return_value=shutil._ntuple_diskusage(
                1000 * vm.GIB, 0, 1000 * vm.GIB)) as usage:
            self.app().preflight()
            usage.assert_called_once_with(self.root)
        self.assertFalse(self.launches())

    def test_fresh_up_creates_exactly_four_and_preserves_source_env(self):
        original = (self.root / ".env").read_bytes()
        self.app().up()
        self.assertEqual(len(self.launches()), 4)
        self.assertEqual((self.root / ".env").read_bytes(), original)
        self.assertIn("AML_BSC_UPSTREAM='http://192.168.64.13:6001'", self.runner.envs["aml-test-main"])
        ups = [call for call in self.runner.calls if "up" in call]
        self.assertEqual([call[call.index("up") - 1] for call in ups], ["tron", "ethereum", "bsc", "main"])
        for call in ups[:3]:
            self.assertIn("--api-only", call)
        checks = [call for call in self.runner.calls if "check-runtime" in call]
        self.assertTrue(checks)
        self.assertTrue(all(call[call.index("check-runtime") - 1] == "main" for call in checks))
        self.assertTrue(all("--pull-never" in call for call in ups))
        self.assertEqual(len([call for call in self.runner.calls if "curl" in call]), 3)
        self.assertFalse(any(call[1] in ("delete", "purge") for call in self.runner.calls))

    def test_each_vm_receives_only_its_own_image_archives(self):
        self.app().up()
        for role in vm.ROLES:
            sent = [call for call in self.runner.calls if call[1] == "transfer" and
                    call[-1].startswith(f"aml-test-{role}:") and call[-1].endswith(".tar")]
            self.assertEqual(len(sent), len(vm.IMAGES[role]))

    def test_repeat_up_reuses_vms_and_owner(self):
        self.app().up()
        owner = self.app().state["owner"]
        self.app().up()
        self.assertEqual(len(self.launches()), 4)
        self.assertEqual(self.app().state["owner"], owner)

    def test_stop_and_restart_retain_all_vms(self):
        self.app().up()
        self.app().stop()
        self.assertTrue(all(item["state"] == "Stopped" for item in self.runner.instances.values()))
        self.app().up()
        self.assertEqual(len(self.launches()), 4)
        self.assertEqual(len([call for call in self.runner.calls if call[1] == "start"]), 4)

    def test_no_adoption_of_existing_names(self):
        self.runner.instances["aml-test-tron"] = {"state": "Running", "owner": {}, "ip": "192.168.64.11"}
        with self.assertRaisesRegex(vm.DeploymentError, "Preflight"):
            self.app().up()
        self.assertFalse(self.launches())

    def test_ownership_mismatch_prevents_deploy_and_stop(self):
        self.app().up()
        self.runner.instances["aml-test-main"]["owner"]["owner"] = "someone-else"
        for action in ("up", "stop"):
            with self.assertRaisesRegex(vm.DeploymentError, "Ownership"):
                getattr(self.app(), action)()

    def test_failure_keeps_state_and_resume_does_not_duplicate(self):
        self.runner.fail_up = "ethereum"
        with self.assertRaisesRegex(vm.DeploymentError, "Injected"):
            self.app().up()
        self.assertEqual(len(self.launches()), 4)
        self.assertTrue((self.root / ".local-vms/state.json").exists())
        self.assertFalse((self.root / ".local-vms/operation.lock").exists())
        self.app().up()
        self.assertEqual(len(self.launches()), 4)

    def test_changed_ip_requires_up_and_then_refreshes_routing(self):
        self.app().up()
        self.runner.instances["aml-test-tron"]["ip"] = "192.168.64.99"
        with self.assertRaisesRegex(vm.DeploymentError, "IPs changed"):
            self.app().check()
        self.app().up()
        self.assertIn("http://192.168.64.99:4001", self.runner.envs["aml-test-main"])

    def test_changed_password_never_silently_reconfigures_database(self):
        self.app().up()
        path = self.root / ".env"
        path.write_text(path.read_text().replace("test-password", "different-password"))
        before = len(self.runner.calls)
        with self.assertRaisesRegex(vm.DeploymentError, "Preflight"):
            self.app().up()
        self.assertFalse(any(call[1] == "transfer" for call in self.runner.calls[before:]))

    def test_lock_prevents_competing_up_or_stop(self):
        app = self.app()
        with app.operation():
            for action in ("up", "stop"):
                with self.assertRaisesRegex(vm.DeploymentError, "Another"):
                    getattr(self.app(), action)()
        self.assertFalse(self.launches())

    def test_ingestion_is_explicit_and_retained_on_resume(self):
        self.args.with_ingestion = True
        self.app().up()
        self.args.with_ingestion = False
        self.app().up()
        self.assertTrue(self.app().state["with_ingestion"])
        self.assertFalse(any("--api-only" in call for call in self.runner.calls))
        checks = [call for call in self.runner.calls if "check-runtime" in call]
        self.assertEqual({call[call.index("check-runtime") - 1] for call in checks}, set(vm.ROLES))
        self.assertFalse(any("check" in call for call in self.runner.calls))

    def test_cloud_init_installs_docker_and_records_owner_without_secrets(self):
        data = json.loads(vm.cloud_config("deployment-id", "tron").split("\n", 1)[1])
        self.assertIn("docker-compose-v2", data["packages"])
        self.assertIn(["systemctl", "enable", "--now", "docker"], data["runcmd"])
        self.assertNotIn("PASSWORD", json.dumps(data))

    def test_local_image_export_cache_tracks_identity_and_checksum(self):
        self.args.bundle = None
        identity = ["sha256:first"]
        saves = []

        def docker(args, **kwargs):
            args = [str(value) for value in args]
            if args[1:3] == ["image", "inspect"]:
                return json.dumps([{"Id": identity[0], "Os": "linux", "Architecture": "amd64"}])
            if args[1:3] == ["image", "save"]:
                path = Path(args[args.index("-o") + 1])
                path.write_bytes((identity[0] + args[-1]).encode())
                saves.append(path)
                return ""
            raise AssertionError(args)

        app = vm.Provisioner(self.args, self.root, docker)
        directory, entries = app.images()
        self.assertEqual(len(saves), 7)
        app.images()
        self.assertEqual(len(saves), 7)
        (directory / entries[vm.ALL_IMAGES[0]]["file"]).write_bytes(b"corrupt")
        app.images()
        self.assertEqual(len(saves), 8)
        identity[0] = "sha256:updated"
        app.images()
        self.assertEqual(len(saves), 15)
        self.assertEqual(len(vm.verify_bundle(directory)), 7)

    def test_wrong_image_platform_is_rejected(self):
        self.args.bundle = None
        app = vm.Provisioner(self.args, self.root, lambda *a, **kw: json.dumps([
            {"Id": "test", "Os": "linux", "Architecture": "arm64"}]))
        with self.assertRaisesRegex(vm.DeploymentError, "linux/amd64"):
            app.inspect_image(vm.ALL_IMAGES[0])

    def test_control_only_targets_one_running_owned_vm(self):
        self.app().up()
        state = (self.root / ".local-vms/state.json").read_bytes()
        for action in vm.CHAIN_ACTIONS:
            self.runner.calls.clear()
            self.app().chain_control("ethereum", action)
            calls = self.runner.calls
            self.assertFalse(any(c[1] in ("launch", "start", "stop", "delete", "purge") for c in calls))
            self.assertTrue(all(c[2] == "aml-test-ethereum" for c in calls if c[1] == "exec"))
            self.assertEqual(calls[-1][-2:], ["ethereum", action])
            self.assertTrue(all(v["state"] == "Running" for v in self.runner.instances.values()))
            self.assertEqual((self.root / ".local-vms/state.json").read_bytes(), state)
            self.assertNotIn(b"\r", (self.root / ".local-vms/control-vm.sh").read_bytes())

    def test_control_query_is_a_single_argument_and_does_not_require_host_docker(self):
        self.app().up()
        sql = "SELECT 'spaces; $(not-a-command)' AS example"
        self.runner.calls.clear()
        with patch.object(vm.shutil, "which", return_value=None):
            self.app().chain_control("bsc", "db", sql)
        self.assertEqual(self.runner.calls[-1][-4:], ["bsc", "db", "--query", sql])
        self.assertTrue(all(c[0] == "multipass" for c in self.runner.calls))
        self.assertNotIn("test-password", json.dumps(self.runner.calls))

    def test_control_refuses_missing_stopped_or_foreign_vm(self):
        with self.assertRaisesRegex(vm.DeploymentError, "No managed"):
            self.app().chain_control("tron", "pause")
        self.app().up()
        self.runner.instances["aml-test-tron"]["state"] = "Stopped"
        self.runner.calls.clear()
        with self.assertRaisesRegex(vm.DeploymentError, "Running"):
            self.app().chain_control("tron", "resume")
        self.assertFalse(any(c[1] != "list" for c in self.runner.calls))
        self.runner.instances["aml-test-tron"]["state"] = "Running"
        self.runner.instances["aml-test-tron"]["owner"]["owner"] = "someone-else"
        self.runner.calls.clear()
        with self.assertRaisesRegex(vm.DeploymentError, "Ownership"):
            self.app().chain_control("tron", "pause")
        self.assertFalse(any(c[1] == "transfer" for c in self.runner.calls))

    def test_control_respects_operation_lock_and_validates_arguments(self):
        self.app().up()
        app = self.app()
        with app.operation(), self.assertRaisesRegex(vm.DeploymentError, "Another"):
            self.app().chain_control("tron", "pause")
        for role, action, query in [("main", "pause", None), ("tron", "delete", None),
                                    ("tron", "pause", "SELECT 1"), ("bsc", "db", " ")]:
            with self.assertRaises(vm.DeploymentError):
                app.chain_control(role, action, query)

    def test_control_read_only_client_does_not_hold_deployment_lock(self):
        self.app().up()
        lock = self.root / ".local-vms/operation.lock"
        original = self.runner

        def runner(args, **kwargs):
            if list(args)[-2:] == ["ethereum", "db"]:
                self.assertFalse(lock.exists())
                self.assertIsNone(kwargs["timeout"])
            return original(args, **kwargs)

        vm.Provisioner(self.args, self.root, runner).chain_control("ethereum", "db")

    def test_control_cli_parses_and_rejects_ambiguous_actions(self):
        args = vm.parser().parse_args(["db", "ethereum", "--query", "SHOW TABLES"])
        self.assertEqual((args.role, args.query), ("ethereum", "SHOW TABLES"))
        for arguments in (["pause"], ["stop", "ethereum"], ["resume", "tron", "--build"],
                          ["ps", "bsc", "--query", "SELECT 1"]):
            with patch.object(vm.sys, "argv", ["provision_vms.py", *arguments]), self.assertRaises(vm.DeploymentError):
                vm.main()

    def test_instance_loaded_before_another_up_reloads_ownership_under_lock(self):
        stale = self.app()
        self.app().up()
        owner = self.app().state["owner"]
        stale.up()
        self.assertEqual(len(self.launches()), 4)
        self.assertEqual(stale.state["owner"], owner)


if __name__ == "__main__":
    unittest.main()
