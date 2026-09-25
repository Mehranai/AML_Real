#!/usr/bin/env python3
"""Create and deploy four owned Multipass VMs. Python 3.10+, standard library only."""
import argparse
import contextlib
import ctypes
import hashlib
import io
import ipaddress
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
ROLES = ("main", "tron", "ethereum", "bsc")
GUEST = "/home/ubuntu/AML_Whole"
GIB = 1024 ** 3
DIRECTORIES = {
    "main": ".", "tron": "dockerizd_tron/app",
    "ethereum": "dockerizd_ethereum", "bsc": "dockerizd_bsc",
}
PORTS = {"tron": 4001, "ethereum": 5001, "bsc": 6001}
IMAGES = {
    "main": ("aml-whole-gateway:local", "aml-analytical-node:local", "neo4j:5.26-community"),
    "tron": ("tron-aml-service:local", "clickhouse/clickhouse-server:23.8"),
    "ethereum": ("ethereum-aml-service:local", "clickhouse/clickhouse-server:23.8"),
    "bsc": ("bsc-aml-service:local", "clickhouse/clickhouse-server:23.8"),
}
ALL_IMAGES = tuple(dict.fromkeys(image for group in IMAGES.values() for image in group))


class DeploymentError(RuntimeError):
    pass


def run(arguments, timeout=300, live=False):
    try:
        result = subprocess.run(
            [str(a) for a in arguments], check=True, timeout=timeout,
            text=True, encoding="utf-8", errors="replace",
            stdout=None if live else subprocess.PIPE,
            stderr=None if live else subprocess.PIPE,
        )
        return result.stdout or ""
    except (OSError, subprocess.TimeoutExpired, subprocess.CalledProcessError) as exc:
        # Never include command arguments: they can contain environment values.
        detail = getattr(exc, "stderr", "") or str(type(exc).__name__)
        raise DeploymentError(f"{arguments[0]} failed: {detail[-1500:]}") from exc


def atomic_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    temporary.chmod(0o600)
    temporary.replace(path)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_env(path):
    values = {}
    if not path.is_file():
        raise DeploymentError(f"Missing ready environment file: {path}")
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        if not separator or not re.fullmatch(r"[A-Z][A-Z0-9_]*", key) or key in values:
            raise DeploymentError(f"Invalid or duplicate env key in {path.name}")
        if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
            value = value[1:-1]
        if any(c in value for c in "\r\n$'") or any(ord(c) < 32 for c in value):
            raise DeploymentError(f"Use literal env values without interpolation: {key}")
        values[key] = value
    return values


def env_text(values):
    return "".join(f"{key}='{value}'\n" for key, value in sorted(values.items()))


def load_config(path):
    config = json.loads(path.read_text(encoding="utf-8-sig"))
    if not re.fullmatch(r"[a-z][a-z0-9-]{0,30}[a-z0-9]", config.get("prefix", "")):
        raise DeploymentError("Invalid VM prefix")
    if config.get("ubuntu") != "24.04" or set(config.get("roles", {})) != set(ROLES):
        raise DeploymentError("Expected Ubuntu 24.04 and exactly main/tron/ethereum/bsc")
    for role, size in config["roles"].items():
        for field, minimum in (("cpus", 1), ("memory_gib", 2), ("disk_gib", 20)):
            if type(size.get(field)) is not int or size[field] < minimum:
                raise DeploymentError(f"Invalid {field} for {role}")
    return config


def source_environments(root):
    environments = {role: read_env(root / DIRECTORIES[role] / ".env") for role in ROLES}
    key = environments["main"].get("AML_SERVICE_KEY", "")
    if len(key) < 32:
        raise DeploymentError("Main AML_SERVICE_KEY must contain at least 32 characters")
    for role, values in environments.items():
        password_key = ("NEO4J_PASSWORD" if role == "main" else
                        "BSC_CLICKHOUSE_PASSWORD" if role == "bsc" else "CLICKHOUSE_PASSWORD")
        minimum = 8 if role == "main" else 1
        if len(values.get(password_key, "")) < minimum:
            raise DeploymentError(f"Missing/short database password in {role} .env")
        if values.get("AML_SERVICE_KEY") != key:
            raise DeploymentError(f"AML_SERVICE_KEY does not match main in {role} .env")
    if environments["main"].get("AML_BASIC_AUTH_REALM", "off") != "off":
        raise DeploymentError("Automatic lab VMs currently require demo UI auth=off; use managed VM deployment for authenticated production")
    return environments


def credential_fingerprint(environments):
    selected = {}
    for role, values in environments.items():
        selected[role] = {key: value for key, value in values.items()
                          if key.endswith("_PASSWORD") or key in ("CLICKHOUSE_USER", "BSC_CLICKHOUSE_USER")}
    return hashlib.sha256(json.dumps(selected, sort_keys=True).encode()).hexdigest()


def guest_environment(role, source, addresses):
    values = dict(source)
    if role == "main":
        values.update(AML_BIND_ADDRESS=addresses["main"], AML_PORT="8080",
                      AML_NEO4J_HTTP_PORT="7474", AML_NEO4J_BOLT_PORT="7687",
                      AML_HTPASSWD_FILE="./gateway/auth/disabled.htpasswd")
        for chain, port in PORTS.items():
            values[f"AML_{chain.upper()}_UPSTREAM"] = f"http://{addresses[chain]}:{port}"
    elif role == "bsc":
        values.update(BSC_API_BIND_ADDRESS=addresses[role], BSC_API_PORT="6001",
                      DOCKER_BIND_ADDRESS="127.0.0.1", BSC_APP_IMAGE_TAG="local",
                      BSC_CLICKHOUSE_IMAGE="clickhouse/clickhouse-server:23.8")
    else:
        values.update(API_BIND_ADDRESS=addresses[role], DATABASE_BIND_ADDRESS="127.0.0.1",
                      AML_SERVICE_AUTH_REQUIRED="true")
        if role == "tron":
            values.update(TRON_API_PORT="4001", TRON_APP_IMAGE_TAG="local")
        else:
            values.update(ETHEREUM_API_PORT="5001", ETHEREUM_APP_IMAGE_TAG="local")
    return values


def cloud_config(owner, role):
    return "#cloud-config\n" + json.dumps({
        "package_update": True,
        "packages": ["docker.io", "docker-compose-v2", "curl", "jq", "ca-certificates", "iptables"],
        "write_files": [{
            "path": "/etc/aml-vm-owner.json", "permissions": "0644",
            "content": json.dumps({"owner": owner, "role": role}),
        }],
        "runcmd": [
            ["systemctl", "enable", "--now", "docker"],
            ["docker", "compose", "version"],
        ],
    }, indent=2) + "\n"


def runtime_archive(root, destination, role, values):
    directory = Path(DIRECTORIES[role])
    compose = directory / ("compose.yaml" if role == "main" else "docker-compose.yml")
    paths = [Path("scripts/vm.sh"), compose]
    if role == "tron":
        paths.append(directory / "sql/init_database_tron.sql")
    if role == "main":
        paths.append(Path("gateway/auth/disabled.htpasswd"))
    with tarfile.open(destination, "w:gz") as archive:
        for relative in paths:
            path = root / relative
            if path.is_symlink() or not path.is_file():
                raise DeploymentError(f"Missing or symlinked runtime file: {relative}")
            # Normalize Windows checkouts; never copy source trees, backups or build caches.
            content = path.read_bytes().replace(b"\r\n", b"\n")
            info = tarfile.TarInfo(relative.as_posix())
            info.mode = 0o644
            info.size = len(content)
            archive.addfile(info, io.BytesIO(content))
        content = env_text(values).encode()
        info = tarfile.TarInfo((directory / ".env").as_posix())
        info.mode = 0o600
        info.size = len(content)
        archive.addfile(info, io.BytesIO(content))
    destination.chmod(0o600)


def verify_bundle(directory):
    entries = json.loads((directory / "manifest.json").read_text(encoding="utf-8-sig"))
    if not isinstance(entries, list) or not entries:
        raise DeploymentError("Invalid image manifest")
    by_image, files = {}, set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise DeploymentError("Invalid image manifest entry")
        image, filename, digest = (entry.get(key, "") for key in ("image", "file", "sha256"))
        if (image not in ALL_IMAGES or image in by_image or filename in files or
                not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*\.tar", filename) or
                not re.fullmatch(r"[a-f0-9]{64}", digest)):
            raise DeploymentError("Invalid, duplicate or unexpected image manifest entry")
        path = directory / filename
        if path.is_symlink() or not path.is_file() or sha256(path) != digest:
            raise DeploymentError(f"Image checksum/missing file: {filename}")
        by_image[image] = entry
        files.add(filename)
    missing = set(ALL_IMAGES) - set(by_image)
    if missing:
        raise DeploymentError(f"Incomplete bundle; missing: {', '.join(sorted(missing))}")
    return by_image


def available_memory():
    if os.name == "nt":
        class Memory(ctypes.Structure):
            _fields_ = [("length", ctypes.c_ulong), ("load", ctypes.c_ulong)] + [
                (name, ctypes.c_ulonglong) for name in (
                    "total", "available", "total_page", "available_page",
                    "total_virtual", "available_virtual", "extended")]
        status = Memory()
        status.length = ctypes.sizeof(status)
        if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
            raise DeploymentError("Cannot inspect host RAM")
        return status.available
    values = dict(line.split(":", 1) for line in Path("/proc/meminfo").read_text().splitlines())
    return int(values["MemAvailable"].split()[0]) * 1024


def existing_parent(path):
    while not path.exists():
        path = path.parent
    return path


class Provisioner:
    def __init__(self, args, root=ROOT, runner=run):
        self.args, self.root, self.run = args, root, runner
        self.config = load_config(args.config)
        self.work = root / ".local-vms"
        self.state_path = self.work / "state.json"
        self.state = json.loads(self.state_path.read_text()) if self.state_path.exists() else None
        if self.state and self.state["config"] != self.config:
            raise DeploymentError("VM configuration changed after creation; restore vms.json. Resize/migration is a separate operation")
        self.names = {role: f"{self.config['prefix']}-{role}" for role in ROLES}
        self.docker = ["docker"] + (["--context", args.docker_context] if args.docker_context else [])

    def mp(self, *arguments, **kwargs):
        return self.run(["multipass", *arguments], **kwargs)

    def guest(self, role, *arguments, **kwargs):
        return self.mp("exec", self.names[role], "--", *arguments, **kwargs)

    def inventory(self):
        return {entry["name"]: entry for entry in json.loads(self.mp("list", "--format", "json"))["list"]}

    @contextlib.contextmanager
    def operation(self):
        self.work.mkdir(parents=True, exist_ok=True)
        self.work.chmod(0o700)
        lock = self.work / "operation.lock"
        try:
            descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError as exc:
            raise DeploymentError("Another deployment is active; inspect .local-vms/operation.lock") from exc
        try:
            with os.fdopen(descriptor, "w") as stream:
                stream.write(str(os.getpid()) + "\n")
            self.state = json.loads(self.state_path.read_text()) if self.state_path.exists() else None
            if self.state and self.state["config"] != self.config:
                raise DeploymentError("VM configuration changed; restore vms.json before resuming")
            yield
        finally:
            lock.unlink()

    def assert_owner(self, role):
        actual = json.loads(self.guest(role, "cat", "/etc/aml-vm-owner.json"))
        if actual != {"owner": self.state["owner"], "role": role}:
            raise DeploymentError(f"Ownership mismatch for {self.names[role]}; refusing to modify it")

    def preflight(self):
        errors = []
        self.environments = source_environments(self.root)
        required = [self.root / "scripts/vm.sh", self.root / "deployment/vm-firewall.sh",
                    self.root / "gateway/auth/disabled.htpasswd",
                    self.root / "dockerizd_tron/app/sql/init_database_tron.sql"]
        required.extend(self.root / directory / ("compose.yaml" if role == "main" else "docker-compose.yml")
                        for role, directory in DIRECTORIES.items())
        if any(not path.is_file() or path.is_symlink() for path in required):
            errors.append("Missing or symlinked runtime files; use a complete project checkout")
        if self.state and self.state["credentials"] != credential_fingerprint(self.environments):
            errors.append("Database credentials changed. Restore source .env or rotate existing databases explicitly")
        if platform.machine().lower() not in ("amd64", "x86_64"):
            errors.append("This image deployment supports Linux/amd64 guests on x86-64 hosts only")
        inventory = {}
        if not shutil.which("multipass"):
            errors.append("Install Multipass first: https://canonical.com/multipass/install")
        else:
            try:
                inventory = self.inventory()
            except DeploymentError as exc:
                errors.append(str(exc))
        if not self.state and any(name in inventory for name in self.names.values()):
            errors.append("VM names already exist without local ownership state; refusing to adopt them")
        for name in self.names.values():
            if name in inventory and inventory[name]["state"] not in ("Running", "Stopped"):
                errors.append(f"{name} is not Running/Stopped; inspect it with multipass info")
        needed_ram = sum(self.config["roles"][role]["memory_gib"] for role in ROLES
                         if inventory.get(self.names[role], {}).get("state") != "Running")
        free_ram = available_memory() / GIB
        if free_ram < needed_ram + 1:
            errors.append(f"Free RAM {free_ram:.1f} GiB; need {needed_ram + 1} GiB to start remaining VMs")
        storage = self.args.storage_path or Path(os.environ.get("SystemDrive", "C:") + "/"
                                                 if os.name == "nt" else "/var/snap/multipass/common")
        needed_disk = 10 + sum(self.config["roles"][role]["disk_gib"] for role in ROLES
                               if self.names[role] not in inventory)
        free_disk = shutil.disk_usage(existing_parent(storage)).free / GIB
        print(f"VM storage check: {storage}; free {free_disk:.1f} GiB, reserve {needed_disk} GiB")
        if free_disk < needed_disk:
            errors.append("Insufficient VM storage. --storage-path checks a custom Multipass storage location; it does NOT move storage")
        staging_gib = 20 if self.args.build else 5
        if shutil.disk_usage(self.root).free < staging_gib * GIB:
            errors.append(f"Need at least {staging_gib} GiB free in the project drive for build/image transfer cache")
        if self.args.bundle:
            try:
                self.bundle_entries = verify_bundle(self.args.bundle)
            except (OSError, ValueError, DeploymentError) as exc:
                errors.append(str(exc))
        else:
            try:
                self.run([*self.docker, "info"], timeout=60)
                if not self.args.build:
                    for image in ALL_IMAGES:
                        self.inspect_image(image)
            except DeploymentError as exc:
                errors.append(str(exc) + "\nProvide --bundle DIRECTORY or use --build with a working Docker daemon")
        for error in errors:
            print(f"[BLOCKED] {error}")
        if errors:
            raise DeploymentError("Preflight failed; no VM has been created or changed")
        print("[OK] Prerequisites verified")
        return inventory

    def inspect_image(self, image):
        data = json.loads(self.run([*self.docker, "image", "inspect", image]))[0]
        if data["Os"] != "linux" or data["Architecture"] != "amd64":
            raise DeploymentError(f"Expected linux/amd64 image: {image}")
        return data["Id"]

    def build_images(self):
        for role in ROLES:
            directory = self.root / DIRECTORIES[role]
            file = directory / ("compose.yaml" if role == "main" else "docker-compose.yml")
            services = ["gateway", "analytical-node"] if role == "main" else [
                {"tron": "tron-api", "ethereum": "ethereum-api", "bsc": "bsc-api"}[role]]
            self.run([*self.docker, "compose", "--project-directory", directory,
                      "--env-file", directory / ".env", "-f", file, "build", *services],
                     timeout=7200, live=True)
        for image in ("clickhouse/clickhouse-server:23.8", "neo4j:5.26-community"):
            self.run([*self.docker, "pull", image], timeout=1800, live=True)

    def images(self):
        if self.args.bundle:
            return self.args.bundle, self.bundle_entries
        if self.args.build:
            self.build_images()
        directory = self.work / "images"
        directory.mkdir(parents=True, exist_ok=True)
        manifest = directory / "manifest.json"
        old = {entry["image"]: entry for entry in json.loads(manifest.read_text())} if manifest.exists() else {}
        entries = {}
        for index, image in enumerate(ALL_IMAGES):
            identity = self.inspect_image(image)
            filename = f"image-{index}.tar"
            path = directory / filename
            entry = old.get(image, {})
            if (entry.get("id") != identity or entry.get("file") != filename or not path.is_file()
                    or sha256(path) != entry.get("sha256")):
                print(f"Exporting {image}")
                temporary = directory / f"image-{index}.partial"
                self.run([*self.docker, "image", "save", "-o", temporary, image], timeout=1800)
                temporary.replace(path)
                entry = {"image": image, "file": filename, "sha256": sha256(path), "id": identity}
            entries[image] = entry
        atomic_json(manifest, list(entries.values()))
        return directory, entries

    def ensure_vm(self, role, inventory):
        name = self.names[role]
        if name not in inventory:
            path = self.work / f"{role}-cloud-init.yaml"
            path.write_text(cloud_config(self.state["owner"], role), encoding="utf-8")
            size = self.config["roles"][role]
            self.mp("launch", self.config["ubuntu"], "--name", name,
                    "--cpus", size["cpus"], "--memory", f"{size['memory_gib']}G",
                    "--disk", f"{size['disk_gib']}G", "--cloud-init", path,
                    "--timeout", "1800", timeout=1900, live=True)
        elif inventory[name]["state"] == "Stopped":
            self.mp("start", name, timeout=600, live=True)
        self.guest(role, "sudo", "cloud-init", "status", "--wait", timeout=1800, live=True)
        self.assert_owner(role)
        self.guest(role, "sudo", "docker", "compose", "version")

    def addresses(self):
        result = {}
        for role in ROLES:
            route = json.loads(self.guest(role, "ip", "-j", "route", "get", "1.1.1.1"))[0]
            address = str(ipaddress.IPv4Address(route["prefsrc"]))
            if ipaddress.ip_address(address).is_loopback:
                raise DeploymentError(f"Invalid VM address for {role}")
            result[role] = address
        if len(set(result.values())) != len(ROLES):
            raise DeploymentError("VM addresses are not unique")
        return result

    def install_firewall(self, role, addresses):
        if role == "main":
            return
        script = self.work / "vm-firewall.sh"
        script.write_bytes((self.root / "deployment/vm-firewall.sh").read_bytes().replace(b"\r\n", b"\n"))
        target = f"{self.names[role]}:/home/ubuntu/aml-vm-firewall.sh"
        self.mp("transfer", script, target)
        self.guest(role, "sudo", "install", "-m", "755", "/home/ubuntu/aml-vm-firewall.sh",
                   "/usr/local/sbin/aml-vm-firewall")
        content = f"MAIN_IP={addresses['main']}\nAPI_PORT={PORTS[role]}\n"
        config = self.work / f"{role}-firewall.env"
        config.write_text(content, encoding="utf-8")
        self.mp("transfer", config, f"{self.names[role]}:/home/ubuntu/firewall.env")
        self.guest(role, "sudo", "install", "-m", "600", "/home/ubuntu/firewall.env", "/etc/aml-vm-firewall.env")
        self.guest(role, "sudo", "bash", "-c",
                   "printf '%s\\n' '[Unit]' 'After=docker.service network-online.target' "
                   "'Requires=docker.service' 'PartOf=docker.service' '[Service]' 'Type=oneshot' "
                   "'ExecStart=/usr/local/sbin/aml-vm-firewall' 'RemainAfterExit=yes' "
                   "'[Install]' 'WantedBy=multi-user.target' > /etc/systemd/system/aml-vm-firewall.service")
        self.guest(role, "sudo", "systemctl", "daemon-reload")
        self.guest(role, "sudo", "systemctl", "enable", "aml-vm-firewall.service")
        self.guest(role, "sudo", "systemctl", "restart", "aml-vm-firewall.service")

    def deploy_role(self, role, addresses, directory, entries):
        self.assert_owner(role)
        self.guest(role, "install", "-d", "-m", "700", GUEST, "/home/ubuntu/aml-upload")
        for image in IMAGES[role]:
            entry = entries[image]
            remote = "/home/ubuntu/aml-upload/" + entry["file"]
            # Each VM receives only its own images, one archive at a time.
            self.mp("transfer", directory / entry["file"], f"{self.names[role]}:{remote}", timeout=1800)
            actual = self.guest(role, "sha256sum", remote).split()[0]
            if actual != entry["sha256"]:
                raise DeploymentError(f"Transferred image checksum mismatch in {role}")
            self.guest(role, "sudo", "docker", "load", "-i", remote, timeout=1800, live=True)
            data = json.loads(self.guest(role, "sudo", "docker", "image", "inspect", image))[0]
            if data["Os"] != "linux" or data["Architecture"] != "amd64":
                raise DeploymentError(f"Unsupported image architecture in {role}")
            self.guest(role, "rm", "--", remote)
        archive = self.work / f"{role}-runtime.tar.gz"
        runtime_archive(self.root, archive, role, guest_environment(role, self.environments[role], addresses))
        remote = "/home/ubuntu/aml-upload/runtime.tar.gz"
        self.mp("transfer", archive, f"{self.names[role]}:{remote}")
        if self.guest(role, "sha256sum", remote).split()[0] != sha256(archive):
            raise DeploymentError(f"Runtime archive checksum mismatch in {role}")
        self.guest(role, "tar", "-xzf", remote, "-C", GUEST, "--no-same-owner")
        self.guest(role, "rm", "--", remote)
        self.install_firewall(role, addresses)
        arguments = ["sudo", "bash", f"{GUEST}/scripts/vm.sh", role, "up", "--pull-never"]
        if role != "main" and not self.state["with_ingestion"]:
            arguments.append("--api-only")
        self.guest(role, *arguments, timeout=900, live=True)
        for attempt in range(30):
            try:
                self.guest(role, "sudo", "bash", f"{GUEST}/scripts/vm.sh", role, "check", timeout=60)
                return
            except DeploymentError:
                if attempt == 29:
                    raise
                time.sleep(4)

    def up(self):
        with self.operation():
            inventory = self.preflight()
            if self.state is None:
                self.state = {"owner": str(uuid.uuid4()), "config": self.config,
                              "credentials": credential_fingerprint(self.environments), "with_ingestion": False}
            self.state["with_ingestion"] |= self.args.with_ingestion
            atomic_json(self.state_path, self.state)
            directory, entries = self.images()
            for role in ROLES:
                print(f"[VM] {self.names[role]}", flush=True)
                self.ensure_vm(role, inventory)
            addresses = self.addresses()
            # Chain readiness must precede main readiness.
            for role in ("tron", "ethereum", "bsc", "main"):
                print(f"[DEPLOY] {role}", flush=True)
                self.deploy_role(role, addresses, directory, entries)
            self.state["addresses"] = addresses
            atomic_json(self.state_path, self.state)
            self.check()

    def check(self):
        if not self.state:
            raise DeploymentError("No managed VMs yet; run up")
        for role in ROLES:
            self.assert_owner(role)
        addresses = self.addresses()
        if addresses != self.state.get("addresses"):
            raise DeploymentError("VM IPs changed; run up to refresh guest .env files and firewall rules")
        for role in ROLES:
            self.guest(role, "sudo", "bash", f"{GUEST}/scripts/vm.sh", role, "check", timeout=60, live=True)
        for role, port in PORTS.items():
            self.guest("main", "curl", "--fail", "--silent", "--show-error", "--connect-timeout", "5",
                       "--max-time", "20", f"http://{addresses[role]}:{port}/ready")
        print(f"Ready: http://{addresses['main']}:8080")
        print("Readiness does not mean blockchain history has been ingested.")

    def stop(self):
        with self.operation():
            self.stop_owned()

    def stop_owned(self):
        if not self.state:
            raise DeploymentError("No managed VM ownership state")
        inventory = self.inventory()
        for role in ROLES:
            if inventory.get(self.names[role], {}).get("state") == "Running":
                self.assert_owner(role)
                self.mp("stop", self.names[role], timeout=600, live=True)
        print("VMs stopped. Disks and database volumes were retained.")


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("action", choices=("doctor", "up", "status", "check", "stop"))
    result.add_argument("--config", type=Path, default=ROOT / "deployment/vms.json")
    result.add_argument("--bundle", type=Path, help="Verified seven-image bundle; host Docker is then unnecessary")
    result.add_argument("--docker-context", default="desktop-linux" if os.name == "nt" else None)
    result.add_argument("--storage-path", type=Path, help="Actual Multipass disk location, for free-space check only")
    result.add_argument("--build", action="store_true", help="Build application images on the host before export")
    result.add_argument("--with-ingestion", action="store_true", help="Enable chain ingestion; can consume substantial disk/RPC")
    return result


def main():
    args = parser().parse_args()
    if args.bundle and args.build:
        raise DeploymentError("--bundle and --build are mutually exclusive")
    if args.action not in ("up", "doctor") and (args.build or args.with_ingestion):
        raise DeploymentError("Build/ingestion options require up")
    app = Provisioner(args)
    if args.action == "doctor":
        app.preflight()
    elif args.action == "up":
        app.up()
    elif args.action == "check":
        app.check()
    elif args.action == "stop":
        app.stop()
    else:
        inventory = app.inventory()
        for role, name in app.names.items():
            entry = inventory.get(name, {})
            print(f"{role}: {entry.get('state', 'Not created')} {entry.get('ipv4', [])}")


if __name__ == "__main__":
    try:
        main()
    except (DeploymentError, OSError, ValueError, KeyError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print("Interrupted. VM disks are retained; run up to resume.", file=sys.stderr)
        sys.exit(130)
