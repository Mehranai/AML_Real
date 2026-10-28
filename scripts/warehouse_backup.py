#!/usr/bin/env python3
"""DB-08A: checkpoint-fenced native backup and verified isolated restore.

This tool never cuts over a runtime, rewrites a legacy row, drops a database or
restores over an existing database. Supply credentials through environment
variables; it never loads .env. Long operations are asynchronous and resumable.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request
import uuid
from contextlib import contextmanager

NETWORKS = {"tron": "tron:mainnet", "ethereum": "eip155:1", "bsc": "eip155:56",
            "bitcoin": "bip122:000000000019d6689c085ae165831e93"}
FORMAT_VERSION = 1
ROW_ENCODING = "typed_hex_v1"
DISK = "aml_backups"


class ClickHouseError(RuntimeError):
    def __init__(self, status, code):
        self.code = code
        super().__init__(f"ClickHouse HTTP {status}, code {code}")


def identifier(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,127}", value):
        raise ValueError("Unsafe database/object identifier")
    return value


def literal(value):
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
        separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def origin(value):
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ("", "/"):
        raise ValueError("Use a credential-free HTTP(S) origin")
    return value.rstrip("/")


class Client:
    def __init__(self, url, user, password, timeout=60):
        self.url = origin(url)
        self.user, self.password, self.timeout = user, password, timeout
        # Do not forward database credentials through environment HTTP proxies.
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def open(self, sql, *, readonly=False, database=None):
        settings = {"max_execution_time": str(self.timeout), "output_format_json_quote_64bit_integers": "1",
                    "output_format_json_quote_denormals": "1", "max_memory_usage": "536870912",
                    "max_bytes_before_external_sort": "67108864"}
        if readonly:
            settings["readonly"] = "2"
        if database is not None:
            settings["database"] = identifier(database)
        request = urllib.request.Request(self.url + "/?" + urllib.parse.urlencode(settings),
            data=sql.encode(), method="POST", headers={"X-ClickHouse-User": self.user, "X-ClickHouse-Key": self.password})
        try:
            return self.opener.open(request, timeout=self.timeout + 5)
        except urllib.error.HTTPError as error:
            # Server text may contain data, URLs or credentials from a DDL.
            body = error.read(512).decode("utf-8", errors="replace")
            code = re.search(r"Code: (\d+)", body)
            raise ClickHouseError(error.code, code[1] if code else "unknown") from None

    def rows(self, sql, *, readonly=True):
        with self.open(sql + " FORMAT JSONEachRow", readonly=readonly) as response:
            result = response.read(32 * 1024 * 1024 + 1)
        if len(result) > 32 * 1024 * 1024:
            raise ValueError("Metadata response exceeded 32 MiB")
        return [json.loads(line) for line in result.decode().splitlines() if line]

    def execute(self, sql, *, database=None):
        with self.open(sql, database=database) as response:
            response.read(65536)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def table_multiset(client, database, name, columns):
    """SHA256 of a sorted, length-delimited typed row representation, including duplicates.

    Native BACKUP preserves parts and exact values. This independent verification
    covers every column in the repository's current scalar/array schemas, not
    just counts or sum(amount). Unsupported state types fail before backup.
    """
    names = ",".join(value_expression("`" + identifier(c["name"]) + "`", c["type"]) for c in columns)
    sha, count = hashlib.sha256(), 0
    amounts = {i: {"column": c["name"], "known_rows": 0, "null_rows": 0, "total": 0}
        for i, c in enumerate(columns) if c["name"] in {"amount", "amount_raw", "value", "value_sats", "fee_paid", "fee_raw", "output_sats", "balance_raw"}
        and re.fullmatch(r"(?:Nullable\()?U?Int(?:8|16|32|64|128|256)\)?", c["type"])}
    sql = f"SELECT toJSONString(tuple({names})) AS row FROM `{database}`.`{name}` ORDER BY row FORMAT JSONEachRow"
    with client.open(sql, readonly=True) as response:
        while line := response.readline(128 * 1024 * 1024 + 1):
            if len(line) > 128 * 1024 * 1024:
                raise ValueError("Verification row exceeded 128 MiB")
            text = json.loads(line)["row"]
            value = text.encode("utf-8")
            sha.update(len(value).to_bytes(8, "big")); sha.update(value); count += 1
            values = json.loads(text)
            for index, amount in amounts.items():
                if values[index] is None:
                    amount["null_rows"] += 1
                else:
                    amount["known_rows"] += 1; amount["total"] += int(values[index])
    for amount in amounts.values():
        amount["total"] = str(amount["total"]) if amount["known_rows"] else None
    return {"rows": count, "multiset_sha256": sha.hexdigest(), "exact_amount_totals": list(amounts.values())}


def value_expression(column, kind):
    """Injective representations for the repository's supported ClickHouse types.

    Binary strings are hex; floats and DateTime64 use their underlying bits.
    NULL, array order and SQL type descriptions remain separate from values.
    """
    if kind.startswith("Nullable(") and kind.endswith(")"):
        inner = value_expression(f"assumeNotNull({column})", kind[9:-1])
        return f"if(isNull({column}),NULL,{inner})"
    if kind.startswith("Array(") and kind.endswith(")"):
        return f"arrayMap(element -> {value_expression('element', kind[6:-1])},{column})"
    if kind in ("String", "LowCardinality(String)") or re.fullmatch(r"FixedString\(\d+\)", kind):
        return f"hex({column})"
    if kind in ("Float32", "Float64"):
        return f"hex(reinterpretAsString({column}))"
    if kind.startswith("DateTime64("):
        return f"toString(reinterpretAsInt64({column}))"
    if kind == "DateTime" or kind.startswith("DateTime("):
        return f"toString(toUInt32({column}))"
    if kind == "Bool":
        return f"toString(reinterpretAsUInt8({column}))"
    if re.fullmatch(r"U?Int(?:8|16|32|64|128|256)", kind):
        return f"toString({column})"
    raise ValueError(f"Unsupported exact verification type: {kind}")


def normalize_sql(sql, database):
    """Normalize database qualifiers only, preserving quoted string literals."""
    pattern = re.compile(r"('(?:\\.|''|[^'])*')|(`(?:``|[^`])*`|[A-Za-z_][A-Za-z0-9_]*)\s*(?=\.)", re.S)
    def replace(match):
        if match[1] is not None:
            return match[0]
        name = match[2].strip("`").replace("``", "`")
        return "{{database}}" if name == database else match[0]
    return pattern.sub(replace, sql)


def render_sql(sql, database):
    identifier(database)
    pattern = re.compile(r"('(?:\\.|''|[^'])*')|\{\{database\}\}(?=\.)", re.S)
    return pattern.sub(lambda m: m[0] if m[1] is not None else "`" + database + "`", sql)


def ordered_views(objects):
    views = {o["name"]: o for o in objects if o["engine"] in ("View", "MaterializedView")}
    dependencies = {}
    for name, obj in views.items():
        sql = re.sub(r"'(?:\\.|''|[^'])*'|--[^\n]*|/\*.*?\*/", " ", obj["ddl"], flags=re.S)
        names = re.findall(r"\b(?:FROM|JOIN|TO)\s+(?:\{\{database\}\}\.)?`?([A-Za-z_][A-Za-z0-9_]*)`?", sql, flags=re.I)
        dependencies[name] = set(names) & views.keys()
    result = []
    while dependencies:
        ready = sorted(name for name, needed in dependencies.items() if not needed)
        if not ready:
            raise ValueError("View dependency cycle cannot be restored safely")
        for name in ready:
            result.append(views[name]); del dependencies[name]
        for needed in dependencies.values():
            needed.difference_update(ready)
    return result


def snapshot(client, database, network, *, schema_only=False):
    identifier(database)
    server = client.rows("SELECT version() AS version, timezone() AS timezone")[0]
    if server["version"].split(".")[:2] != ["23", "8"]:
        raise ValueError("Backup/restore is validated only on ClickHouse 23.8")
    objects = client.rows("SELECT name,engine,create_table_query FROM system.tables WHERE database=" + literal(database) + " ORDER BY name")
    if not objects:
        raise ValueError("Source database is missing or empty")
    columns = client.rows("SELECT table,name,type,position,default_kind,default_expression,compression_codec,comment FROM system.columns WHERE database=" + literal(database) + " ORDER BY table,position")
    identity = None
    binding = "unverified_legacy"
    if any(o["name"] == "aml_warehouse_identity" for o in objects):
        identity = client.rows(f"SELECT network_id,schema_version,checksum FROM `{database}`.aml_warehouse_identity FINAL")
        if not (schema_only and not identity) and (len(identity) != 1 or identity[0]["network_id"] != NETWORKS[network]):
            raise ValueError("Warehouse network identity mismatch")
        binding = "shared_identity_verified" if identity else "restore_empty_schema"
    for name in ("ingested_blocks", "sync_state", "ingestion_state"):
        names = {c["name"] for c in columns if c["table"] == name}
        column = "network_id" if "network_id" in names else "chain" if "chain" in names else None
        if column:
            values = client.rows(f"SELECT DISTINCT `{column}` AS identity FROM `{database}`.`{name}` LIMIT 2")
            expected = NETWORKS[network] if column == "network_id" else network
            if values and any(row["identity"] != expected for row in values):
                raise ValueError("Legacy network identity mismatch")
            if values and binding != "shared_identity_verified":
                binding = "legacy_row_verified"
    tables = []
    for obj in objects:
        name = identifier(obj["name"])
        if obj["engine"] not in ("View", "MaterializedView", "MergeTree", "ReplacingMergeTree", "SummingMergeTree"):
            raise ValueError(f"Unsupported backup verification engine: {obj['engine']}")
        item = {"name": name, "engine": obj["engine"],
                "ddl": normalize_sql(obj["create_table_query"], database),
                "columns": [{k: v for k, v in c.items() if k != "table"} for c in columns if c["table"] == name]}
        if not schema_only and obj["engine"] not in ("View", "MaterializedView"):
            stored = [c for c in item["columns"] if c["default_kind"] != "ALIAS"]
            item["data"] = table_multiset(client, database, name, stored)
        tables.append(item)
    # Include all raw state rows in the table digests: legacy checkpoints,
    # orphan markers, review versions and unpublished attempts are preserved.
    return {"server": server, "encoding": ROW_ENCODING, "network": network, "network_id": NETWORKS[network],
            "identity": identity, "network_binding": binding, "objects": tables, "sha256": digest(tables)}


def stable_snapshot(client, database, network):
    before = parts_signature(client, database)
    value = snapshot(client, database, network)
    if parts_signature(client, database) != before:
        raise ValueError("Source parts changed during snapshot; pause writers/merges and retry")
    return value


def parts_signature(client, database):
    parts = client.rows("SELECT table,name,rows,bytes_on_disk,modification_time FROM system.parts WHERE active AND database=" + literal(database) + " ORDER BY table,name")
    mutations = client.rows("SELECT table,mutation_id,command,is_done FROM system.mutations WHERE database=" + literal(database) + " ORDER BY table,mutation_id")
    return digest({"parts": parts, "mutations": mutations})


def load(path):
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if data.get("format_version") != FORMAT_VERSION:
        raise ValueError("Unsupported operation manifest")
    if data.get("kind") not in ("backup", "restore") or data.get("network") not in NETWORKS:
        raise ValueError("Invalid operation kind/network")
    identifier(data["database"]); origin(data["endpoint"]); uuid.UUID(data["operation_id"])
    source = data["source"]
    if source.get("encoding") != ROW_ENCODING or source["network"] != data["network"] or source["network_id"] != NETWORKS[data["network"]] or digest(source["objects"]) != source["sha256"]:
        raise ValueError("Source snapshot integrity mismatch")
    if data["kind"] == "backup" and (data["disk"] != DISK or data["archive"] != f"{data['network']}-{data['operation_id']}.zip"):
        raise ValueError("Unsafe backup location")
    if data["kind"] == "restore":
        identifier(data["backup_database"]); uuid.UUID(data["data_operation_id"]); uuid.UUID(data["backup_operation_id"])
        if data.get("phase") not in ("structure", "data") or data["archive"] != f"{data['network']}-{data['backup_operation_id']}.zip":
            raise ValueError("Invalid restore phase or location")
    if data.get("status") == "verified" and data.get("verified_sha256") != source["sha256"]:
        raise ValueError("Verified manifest seal does not match its source")
    return data


@contextmanager
def operation_lock(path):
    path = Path(path).with_name(Path(path).name + ".lock")
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as handle:
        if handle.tell() == 0:
            handle.write(b"0"); handle.flush()
        handle.seek(0)
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            yield
        finally:
            handle.seek(0)
            if os.name == "nt":
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def save(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(data, handle, ensure_ascii=False, indent=2, allow_nan=False)
        handle.write("\n"); handle.flush(); os.fsync(handle.fileno())
    temporary.replace(path)


def operation(client, operation_id):
    uuid.UUID(operation_id)
    result = client.rows("SELECT toString(id) AS id,name,toString(status) AS status,error FROM system.backups WHERE id=" + literal(operation_id))
    return result[0] if result else None


def backup(client, database, network, path, *, paused, resume=False):
    if not paused:
        raise ValueError("Pause network writers/workers before backup; --writers-paused is required")
    if Path(path).exists():
        if not resume:
            raise ValueError("Operation manifest already exists; use --resume")
        data = load(path)
        if data.get("kind") != "backup" or data["database"] != database or data["network"] != network or data["endpoint"] != client.url:
            raise ValueError("Resume source does not match this operation")
        if data["status"] in ("verified", "rejected"):
            return data
    else:
        if not client.rows("SELECT name FROM system.disks WHERE name=" + literal(DISK)):
            raise ValueError("Configure the independent aml_backups disk before backup")
        source_guard = parts_signature(client, database)
        source = stable_snapshot(client, database, network)
        if parts_signature(client, database) != source_guard:
            raise ValueError("Source changed during backup preflight")
        operation_id = str(uuid.uuid4())
        data = {"format_version": FORMAT_VERSION, "kind": "backup", "operation_id": operation_id,
                "endpoint": client.url, "database": identifier(database), "network": network,
                "archive": f"{network}-{operation_id}.zip", "disk": DISK,
                "status": "prepared", "source": source, "source_parts_sha256": source_guard}
        save(path, data)
    job = operation(client, data["operation_id"])
    if job is None:
        if data["status"] != "prepared":
            raise ValueError("Operation state unavailable; do not assume an existing archive is complete")
        if parts_signature(client, database) != data["source_parts_sha256"]:
            data["status"] = "rejected"; data["reason"] = "source_changed"; save(path, data)
            return data
        # Persist dispatch before I/O. An uncertain response must be reconciled,
        # never retried as a new write to the same archive.
        data["status"] = "dispatched"; save(path, data)
        try:
            client.execute(f"BACKUP DATABASE `{database}` TO Disk('{DISK}',{literal(data['archive'])}) SETTINGS id={literal(data['operation_id'])} ASYNC")
        except ClickHouseError as error:
            if operation(client, data["operation_id"]) is None:
                data["status"] = "rejected"; data["reason"] = "server_rejected"; data["server_error_code"] = error.code; save(path, data)
                return data
        except OSError:
            return data
    return refresh(client, path)


def refresh(client, path):
    data = load(path)
    if data["endpoint"] != client.url:
        raise ValueError("Operation endpoint mismatch")
    if data["status"] in ("verified", "rejected"):
        return data
    active_id = data["data_operation_id"] if data["kind"] == "restore" and data["phase"] == "data" else data["operation_id"]
    job = operation(client, active_id)
    if job is None:
        raise ValueError("Operation status is unavailable; recovery requires a new isolated attempt")
    state = job["status"]
    data["server_status"] = state
    if state in ("BACKUP_FAILED", "RESTORE_FAILED"):
        data["status"] = "rejected"; data["reason"] = "server_operation_failed"
    elif state in ("BACKUP_CREATED", "RESTORED"):
        if data["kind"] == "restore" and data["phase"] == "structure":
            prepare_restore_data(client, data)
            data["phase"] = "data"; data["status"] = "dispatched"; data["merges"] = "paused_for_verification"; save(path, data)
            return dispatch_restore(client, data, path)
        database = data["database"]
        actual = stable_snapshot(client, database, data["network"])
        expected = data["source"]
        changed_parts = data["kind"] == "backup" and parts_signature(client, database) != data["source_parts_sha256"]
        if actual != expected or changed_parts:
            data["status"] = "rejected"; data["reason"] = "source_changed" if data["kind"] == "backup" else "restored_content_differs"
            data["actual_sha256"] = actual["sha256"]
        else:
            data["status"] = "verified"
            data["verified_sha256"] = actual["sha256"]
    else:
        data["status"] = "running"
    save(path, data)
    return data


def restore(client, backup_path, target, path, *, resume=False):
    source = load(backup_path)
    if source.get("kind") != "backup" or source.get("status") != "verified":
        raise ValueError("A verified backup manifest is required")
    identifier(target)
    if source["disk"] != DISK or not re.fullmatch(r"(?:tron|ethereum|bsc|bitcoin)-[0-9a-f-]{36}\.zip", source["archive"]):
        raise ValueError("Unsafe backup location")
    if Path(path).exists():
        if not resume:
            raise ValueError("Restore manifest exists; use --resume")
        data = load(path)
        if data.get("kind") != "restore" or data["database"] != target or data["endpoint"] != client.url or data["backup_manifest_sha256"] != digest(source):
            raise ValueError("Restore resume does not match this operation")
        return refresh(client, path)
    server = client.rows("SELECT version() AS version, timezone() AS timezone")[0]
    if server != source["source"]["server"]:
        raise ValueError("Restore requires the same ClickHouse version and timezone")
    existing = client.rows("SELECT name FROM system.databases WHERE name=" + literal(target))
    if existing:
        raise ValueError("Restore target must not exist; existing data is never overwritten")
    data = {"format_version": FORMAT_VERSION, "kind": "restore", "operation_id": str(uuid.uuid4()),
            "endpoint": client.url, "database": target, "network": source["network"], "status": "dispatched",
            "phase": "structure", "data_operation_id": str(uuid.uuid4()), "backup_operation_id": source["operation_id"],
            "archive": source["archive"], "backup_database": source["database"],
            "source": source["source"], "backup_manifest_sha256": digest(source)}
    save(path, data)
    return dispatch_restore(client, data, path)


def prepare_restore_data(client, data):
    target = identifier(data["database"])
    actual = snapshot(client, target, data["network"], schema_only=True)
    expected = [{k: v for k, v in t.items() if k != "data"} for t in data["source"]["objects"]]
    by_name = {t["name"]: t for t in actual["objects"]}
    if set(by_name) - {t["name"] for t in expected}:
        raise ValueError("Unexpected objects in isolated restore")
    views = ordered_views(expected)
    for table in expected:
        restored = by_name.get(table["name"])
        if table["engine"] not in ("View", "MaterializedView") and restored != table:
            raise ValueError("Restored physical structure does not match the source")
        if restored is not None and (restored["columns"] != table["columns"] or restored["engine"] != table["engine"]):
            raise ValueError("Restored object type/columns differ from source")
    for table in expected:
        if table["engine"] not in ("View", "MaterializedView"):
            name = identifier(table["name"])
            count = client.rows(f"SELECT toString(count()) AS n FROM `{target}`.`{name}`")[0]["n"]
            if count != "0":
                raise ValueError("Structure-only restore target contains unexpected rows")
            client.execute(f"SYSTEM STOP MERGES `{target}`.`{name}`")
    # ClickHouse 23.8 DATABASE AS can lose table aliases in restored view ASTs.
    # Rebuild only the new, empty target's views from the captured source DDL.
    # Physical tables are never dropped/replaced, and all schemas are checked
    # again before data restore. Missing views from an interrupted repair resume.
    for view in views:
        name = identifier(view["name"])
        prefix = "CREATE MATERIALIZED VIEW " if view["engine"] == "MaterializedView" else "CREATE VIEW "
        if not view["ddl"].startswith(prefix + "{{database}}." + name + " "):
            raise ValueError("Unexpected view declaration")
        sql = render_sql(view["ddl"], target)
        if view["engine"] == "MaterializedView":
            if not re.search(r"\bTO\s+\{\{database\}\}\.", view["ddl"]):
                raise ValueError("Materialized views with implicit private storage are unsupported")
            if name in by_name:
                client.execute(f"DROP VIEW `{target}`.`{name}`")
        else:
            sql = sql.replace("CREATE VIEW ", "CREATE OR REPLACE VIEW ", 1)
        client.execute(sql, database=target)
    if snapshot(client, target, data["network"], schema_only=True)["objects"] != expected:
        raise ValueError("Reconstructed view definitions do not match source")


def dispatch_restore(client, data, path):
    structure = data["phase"] == "structure"
    operation_id = data["operation_id"] if structure else data["data_operation_id"]
    # Native metadata comparison has the same alias-loss issue. Only after our
    # independent exact schema validation may the data phase retain fixed views.
    options = ",structure_only=true" if structure else ",allow_non_empty_tables=false,allow_different_table_def=true"
    try:
        client.execute(f"RESTORE DATABASE `{data['backup_database']}` AS `{data['database']}` FROM Disk('{DISK}',{literal(data['archive'])}) SETTINGS id={literal(operation_id)}{options} ASYNC")
    except ClickHouseError as error:
        if operation(client, operation_id) is None:
            data["status"] = "rejected"; data["reason"] = "server_rejected"; data["server_error_code"] = error.code; save(path, data)
            return data
    except OSError:
        return data
    return refresh(client, path)


def release_restored_merges(client, path):
    data = load(path)
    if data["kind"] != "restore" or data["status"] != "verified" or data["endpoint"] != client.url:
        raise ValueError("Only a verified restore on its recorded endpoint can release merges")
    if data.get("merges") == "resumed":
        return data
    if stable_snapshot(client, data["database"], data["network"]) != data["source"]:
        raise ValueError("Restored data changed before release; do not use it for cutover")
    for table in data["source"]["objects"]:
        if "data" in table:
            client.execute(f"SYSTEM START MERGES `{identifier(data['database'])}`.`{identifier(table['name'])}`")
    data["merges"] = "resumed"; save(path, data)
    return data


def public_status(data):
    return {k: data[k] for k in ("kind", "operation_id", "phase", "data_operation_id", "database", "network", "status", "server_status", "reason", "verified_sha256", "merges") if k in data}


def compare_receipts(backup_path, restore_path):
    source, restored = load(backup_path), load(restore_path)
    if source["kind"] != "backup" or restored["kind"] != "restore" or source["status"] != "verified" or restored["status"] != "verified":
        raise ValueError("Both native backup and isolated restore must be verified")
    if restored["backup_manifest_sha256"] != digest(source) or source["source"] != restored["source"]:
        raise ValueError("Restore is not bound to this exact backup manifest")
    return {"network": source["network"], "backup_operation_id": source["operation_id"],
            "restore_operation_id": restored["operation_id"], "source_sha256": source["source"]["sha256"],
            "objects": len(source["source"]["objects"]),
            "physical_tables": sum("data" in t for t in source["source"]["objects"]),
            "rows": sum(t.get("data", {}).get("rows", 0) for t in source["source"]["objects"]),
            "restore_verified": True, "cutover_allowed": False,
            "remaining": ["lossless source-to-target mapping", "backfill reconciliation", "active writer/reader and worker parity"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url")
    parser.add_argument("--user-env", default="AML_BACKUP_USER")
    parser.add_argument("--password-env", default="AML_BACKUP_PASSWORD")
    parser.add_argument("--timeout", type=int, default=60)
    commands = parser.add_subparsers(dest="command", required=True)
    take = commands.add_parser("backup")
    take.add_argument("--network", choices=NETWORKS, required=True)
    take.add_argument("--database", required=True)
    take.add_argument("--manifest", required=True)
    take.add_argument("--writers-paused", action="store_true")
    take.add_argument("--resume", action="store_true")
    check = commands.add_parser("status")
    check.add_argument("--manifest", required=True)
    recover = commands.add_parser("restore")
    recover.add_argument("--backup-manifest", required=True)
    recover.add_argument("--target-database", required=True)
    recover.add_argument("--manifest", required=True)
    recover.add_argument("--resume", action="store_true")
    verify = commands.add_parser("verify", help="offline: bind verified backup and restore receipts; never authorizes cutover")
    verify.add_argument("--backup-manifest", required=True)
    verify.add_argument("--restore-manifest", required=True)
    release = commands.add_parser("release-merges", help="release only the verified isolated restore's merges; no runtime cutover")
    release.add_argument("--manifest", required=True)
    args = parser.parse_args()
    if args.command == "verify":
        print(json.dumps(compare_receipts(args.backup_manifest, args.restore_manifest), sort_keys=True))
        return 0
    if not args.url:
        parser.error("--url is required for server operations")
    if not 1 <= args.timeout <= 3600:
        parser.error("timeout must be 1..3600")
    user, password = os.environ.get(args.user_env), os.environ.get(args.password_env)
    if not user or not password:
        parser.error("Set the named credential environment variables; .env is never read")
    client = Client(args.url, user, password, args.timeout)
    with operation_lock(args.manifest):
        if args.command == "backup":
            data = backup(client, args.database, args.network, args.manifest, paused=args.writers_paused, resume=args.resume)
        elif args.command == "restore":
            data = restore(client, args.backup_manifest, args.target_database, args.manifest, resume=args.resume)
        elif args.command == "release-merges":
            data = release_restored_merges(client, args.manifest)
        else:
            data = refresh(client, args.manifest)
    print(json.dumps(public_status(data), sort_keys=True))
    return 0 if data["status"] == "verified" else 2 if data["status"] == "rejected" else 3


if __name__ == "__main__":
    raise SystemExit(main())
