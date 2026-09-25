"""Local launcher tests; Docker and databases are not started."""
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("run_local", Path(__file__).parents[1] / "run_local.py")
local = importlib.util.module_from_spec(spec)
spec.loader.exec_module(local)


class LocalRunnerTests(unittest.TestCase):
    def setUp(self):
        self.commands = []

    def docker(self, command, **kwargs):
        self.commands.append((command, kwargs.get("env")))
        result = ""
        if "inspect" in command:
            name = command[-1]
            service = "neo4j" if name.endswith("-neo4j-1") else "clickhouse"
            project = name.removesuffix("-" + service + "-1")
            result = json.dumps([{"Config": {
                "Labels": {"com.docker.compose.project": project, "com.docker.compose.service": service},
                "Env": ["CLICKHOUSE_USER=existing-user", "CLICKHOUSE_PASSWORD=existing-password",
                        "NEO4J_AUTH=neo4j/existing-neo4j-password"],
            }}])
        if "config" in command:
            result = json.dumps({"services": {"analytical-node": {"environment": {"AML_SERVICE_KEY": "test-" * 10}}}})
        return subprocess.CompletedProcess(command, 0, stdout=result, stderr="")

    def test_preserves_credentials_and_sets_only_process_local_routing(self):
        with patch.object(local.subprocess, "run", side_effect=self.docker), redirect_stdout(io.StringIO()):
            runner = local.LocalRunner()
        self.assertEqual(runner.environments["tron"]["CLICKHOUSE_PASSWORD"], "existing-password")
        self.assertEqual(runner.environments["bsc"]["BSC_CLICKHOUSE_PASSWORD"], "existing-password")
        self.assertEqual(runner.environments["main"]["NEO4J_PASSWORD"], "existing-neo4j-password")
        self.assertEqual(runner.environments["main"]["AML_ETHEREUM_UPSTREAM"], "http://host.docker.internal:15001")
        self.assertEqual(runner.environments["ethereum"]["ETHEREUM_API_PORT"], "15001")
        for environment in runner.environments.values():
            self.assertEqual(environment["AML_SERVICE_KEY"], "test-" * 10)
            self.assertEqual(environment["API_BIND_ADDRESS"], "127.0.0.1")

    def test_up_only_starts_apis_and_central_gateway_without_deleting_data(self):
        with patch.object(local.subprocess, "run", side_effect=self.docker), redirect_stdout(io.StringIO()):
            runner = local.LocalRunner()
            with patch.object(runner, "check") as check:
                runner.up()
                check.assert_called_once_with(wait=True)
        starts = [command for command, _ in self.commands if "up" in command]
        self.assertEqual([command[-1] for command in starts], ["tron-api", "ethereum-api", "bsc-api", "gateway"])
        for command in starts:
            self.assertIn("--no-build", command)
            self.assertIn("never", command)
        self.assertFalse(any("down" in command or "--remove-orphans" in command for command, _ in self.commands))

    def test_health_checks_ignore_system_proxy(self):
        with patch.object(local.subprocess, "run", side_effect=self.docker), redirect_stdout(io.StringIO()):
            runner = local.LocalRunner()
            with patch.object(local.urllib.request, "build_opener") as build:
                build.return_value.open.return_value.__enter__.side_effect = [
                    io.BytesIO(b'{"status":"ready"}') for _ in range(4)]
                runner.check()
                self.assertEqual(build.call_args.args[0].proxies, {})
                self.assertEqual(build.return_value.open.call_count, 4)

    def test_full_start_includes_all_workers_and_bsc_runtime_profile(self):
        with patch.object(local.subprocess, "run", side_effect=self.docker), redirect_stdout(io.StringIO()):
            runner = local.LocalRunner()
            with patch.object(runner, "check"), patch.object(runner, "check_workers") as workers:
                runner.up(with_ingestion=True, build=True)
                workers.assert_called_once_with(wait=True)
        starts = [command for command, _ in self.commands if "up" in command]
        for command, role in zip(starts, local.STACKS):
            self.assertIn("--build", command)
            self.assertNotIn("--no-build", command)
            for service in local.WORKERS.get(role, ()):
                self.assertIn(service, command)
        self.assertIn("--profile", starts[2])
        self.assertIn("runtime", starts[2])

    def test_worker_check_rejects_missing_or_unhealthy_workers(self):
        with patch.object(local.subprocess, "run", side_effect=self.docker), redirect_stdout(io.StringIO()):
            runner = local.LocalRunner()
            with patch.object(runner, "compose", return_value="[]"):
                with self.assertRaisesRegex(RuntimeError, "workers not running/healthy"):
                    runner.check_workers()
            def status(role, *args, **kwargs):
                return json.dumps([{"Service": name, "State": "running", "Health": "healthy"}
                                   for name in local.WORKERS[role]])
            with patch.object(runner, "compose", side_effect=status):
                runner.check_workers()


if __name__ == "__main__":
    unittest.main()
