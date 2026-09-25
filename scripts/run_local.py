#!/usr/bin/env python3
"""Run the existing AML containers locally, without creating virtual machines."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
STACKS = {
    "tron": ("app", "dockerizd_tron/app", "docker-compose.yml", "tron-api"),
    "ethereum": ("dockerizd_ethereum", "dockerizd_ethereum", "docker-compose.yml", "ethereum-api"),
    "bsc": ("dockerizd_bsc", "dockerizd_bsc", "docker-compose.yml", "bsc-api"),
    "main": ("aml-whole", ".", "compose.yaml", "gateway"),
}
WORKERS = {
    "tron": ("tron-ingestion", "tron-token-metadata-worker", "tron-analytics"),
    "ethereum": ("ethereum-ingestion", "ethereum-token-metadata", "ethereum-analytics"),
    "bsc": ("bsc-follow", "bsc-token-metadata"),
}


class LocalRunner:
    def __init__(self):
        self.docker = ["docker"] + (["--context", "desktop-linux"] if os.name == "nt" else [])
        common = dict(os.environ)
        common.update(
            AML_BIND_ADDRESS="127.0.0.1", AML_PORT="8080",
            API_BIND_ADDRESS="127.0.0.1", BSC_API_BIND_ADDRESS="127.0.0.1",
            DATABASE_BIND_ADDRESS="127.0.0.1", DOCKER_BIND_ADDRESS="127.0.0.1",
            TRON_API_PORT="4001", ETHEREUM_API_PORT="15001", BSC_API_PORT="6001",
            AML_TRON_UPSTREAM="http://host.docker.internal:4001",
            AML_ETHEREUM_UPSTREAM="http://host.docker.internal:15001",
            AML_BSC_UPSTREAM="http://host.docker.internal:6001",
            AML_SERVICE_AUTH_REQUIRED="true",
        )
        self.environments = {role: dict(common) for role in STACKS}
        self.call([*self.docker, "info"], capture=True)
        for role, (project, _, _, _) in STACKS.items():
            service = "neo4j" if role == "main" else "clickhouse"
            result = subprocess.run(
                [*self.docker, "inspect", f"{project}-{service}-1"],
                text=True, encoding="utf-8", capture_output=True,
            )
            if result.returncode:
                continue
            container = json.loads(result.stdout)[0]
            labels = container["Config"].get("Labels", {})
            if labels.get("com.docker.compose.project") != project or labels.get("com.docker.compose.service") != service:
                raise RuntimeError(f"Unexpected existing container ownership: {project}/{service}")
            previous = dict(item.split("=", 1) for item in container["Config"]["Env"] if "=" in item)
            if role == "main":
                auth = previous.get("NEO4J_AUTH", "")
                if auth.startswith("neo4j/"):
                    self.environments[role]["NEO4J_PASSWORD"] = auth.split("/", 1)[1]
            else:
                prefix = "BSC_" if role == "bsc" else ""
                for key in ("CLICKHOUSE_USER", "CLICKHOUSE_PASSWORD"):
                    if key in previous:
                        self.environments[role][prefix + key] = previous[key]
            print(f"{role}: retaining existing database container credentials", flush=True)
        # Use the central service key for all APIs, without editing any .env file.
        central = self.config("main")
        key = central["services"]["analytical-node"]["environment"]["AML_SERVICE_KEY"]
        for environment in self.environments.values():
            environment["AML_SERVICE_KEY"] = key

    @staticmethod
    def call(command, environment=None, capture=False):
        result = subprocess.run(
            [str(value) for value in command], env=environment, check=True,
            text=True, encoding="utf-8", errors="replace",
            stdout=subprocess.PIPE if capture else None,
        )
        return result.stdout or ""

    def compose(self, role, *arguments, capture=False):
        project, directory, filename, _ = STACKS[role]
        path = ROOT / directory
        return self.call(
            [*self.docker, "compose", "-p", project, "--project-directory", path,
             "--env-file", path / ".env", "-f", path / filename, *arguments],
            self.environments[role], capture=capture,
        )

    def config(self, role):
        return json.loads(self.compose(role, "config", "--format", "json", capture=True))

    def up(self, with_ingestion=False, build=False):
        for role in STACKS:
            self.config(role)
        for role, (_, _, _, service) in STACKS.items():
            mode = "ingestion and automatic workers" if with_ingestion and role != "main" else "API"
            print(f"Starting {role} ({mode})...", flush=True)
            profile = ("--profile", "runtime") if role == "bsc" and with_ingestion else ()
            options = ("--build",) if build else ("--no-build", "--pull", "never")
            workers = WORKERS.get(role, ()) if with_ingestion else ()
            self.compose(role, *profile, "up", "-d", *options, service, *workers)
        self.check(wait=True)
        if with_ingestion:
            self.check_workers(wait=True)

    def check_workers(self, wait=False):
        for role, workers in WORKERS.items():
            for attempt in range(90 if wait else 1):
                raw = self.compose(role, "ps", "-a", "--format", "json", capture=True).strip()
                rows = json.loads(raw) if raw.startswith("[") else [json.loads(line) for line in raw.splitlines()]
                by_service = {row["Service"]: row for row in rows}
                missing = [name for name in workers if by_service.get(name, {}).get("State") != "running"
                           or by_service.get(name, {}).get("Health", "") not in ("", "healthy")]
                if not missing:
                    print(f"PASS {role} worker processes: {', '.join(workers)}", flush=True)
                    break
                if not wait or attempt == 89:
                    raise RuntimeError(f"{role} workers not running/healthy: {', '.join(missing)}. Inspect container logs.")
                time.sleep(2)

    def check(self, wait=False):
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        for route in ("/ready", "/networks/tron/ready", "/networks/ethereum/ready", "/networks/bsc/ready"):
            for attempt in range(90 if wait else 1):
                try:
                    with opener.open("http://127.0.0.1:8080" + route, timeout=10) as response:
                        data = json.load(response)
                    if data.get("status") != "ready":
                        raise RuntimeError(f"Not ready: {route}")
                    print("PASS " + route, flush=True)
                    break
                except (OSError, ValueError, RuntimeError):
                    if not wait or attempt == 89:
                        raise
                    time.sleep(2)
        print("Open http://127.0.0.1:8080", flush=True)
        print("API readiness alone does not certify ingestion progress or complete analysis coverage.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", nargs="?", default="up", choices=("up", "check", "status", "stop"))
    parser.add_argument("--with-ingestion", action="store_true", help="start ingestion and automatic workers for all three networks")
    parser.add_argument("--build", action="store_true", help="rebuild images before startup")
    args = parser.parse_args()
    if args.action != "up" and (args.with_ingestion or args.build):
        parser.error("--with-ingestion and --build require up")
    runner = LocalRunner()
    if args.action == "up":
        runner.up(with_ingestion=args.with_ingestion, build=args.build)
    elif args.action == "check":
        runner.check()
    else:
        roles = reversed(STACKS) if args.action == "stop" else STACKS
        for role in roles:
            runner.compose(role, *(("stop",) if args.action == "stop" else ("ps", "-a")))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Local startup failed: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print("Interrupted. Existing containers and data were not deleted.", file=sys.stderr)
        sys.exit(130)
