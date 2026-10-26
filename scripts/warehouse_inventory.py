#!/usr/bin/env python3
"""Materialize legacy DDL in an empty disposable ClickHouse and export its schema.

Never reads .env or inspects a deployed database. Creates only aml_inventory_*
databases, refuses existing objects, never drops data, and uses explicit test
credentials. Source references are audit leads, not proof a feature is active.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CHAINS = {
    "tron": ("dockerizd_tron/app", ["sql/init_database_tron.sql"]),
    "ethereum": ("dockerizd_ethereum", None),
    "bsc": ("dockerizd_bsc", None),
    "bitcoin": ("dockerizd_bitcoin", ["sql/legacy/001_core.sql"]),
}


def statements(sql):
    """Split DDL without splitting semicolons inside strings or comments."""
    result, current, state, i = [], [], None, 0
    while i < len(sql):
        c = sql[i]
        following = sql[i + 1] if i + 1 < len(sql) else ""
        if state == "line":
            if c == "\n":
                state = None
                current.append("\n")
        elif state == "block":
            if c == "*" and following == "/":
                state = None
                current.append(" ")
                i += 1
        elif state:
            current.append(c)
            if c == "\\" and following:
                current.append(following)
                i += 1
            elif c == state:
                if following == state:
                    current.append(following)
                    i += 1
                else:
                    state = None
        elif c == "-" and following == "-":
            state = "line"
            i += 1
        elif c == "/" and following == "*":
            state = "block"
            i += 1
        elif c in "'\"`":
            state = c
            current.append(c)
        elif c == ";":
            if "".join(current).strip():
                result.append("".join(current).strip())
            current = []
        else:
            current.append(c)
        i += 1
    if state not in (None, "line"):
        raise ValueError("Unterminated SQL literal or block comment")
    if "".join(current).strip():
        result.append("".join(current).strip())
    return result


class TestClickHouse:
    def __init__(self, url, user, password):
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme not in ("http", "https") or parsed.hostname not in ("127.0.0.1", "localhost"):
            raise ValueError("Inventory must use a disposable ClickHouse bound to localhost")
        if parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ("", "/"):
            raise ValueError("Use a credential-free ClickHouse origin")
        self.url, self.user, self.password = url, user, password
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def execute(self, sql, database=None, rows=False):
        args = {"max_execution_time": "120"}
        if database:
            if not re.fullmatch(r"aml_inventory_[a-z]+", database):
                raise ValueError("Inventory database prefix is fixed")
            args["database"] = database
        if rows:
            sql += " FORMAT JSONEachRow"
        request = urllib.request.Request(self.url.rstrip("/") + "/?" + urllib.parse.urlencode(args),
            data=sql.encode("utf-8"), headers={"X-ClickHouse-User": self.user,
                "X-ClickHouse-Key": self.password}, method="POST")
        with self.opener.open(request, timeout=125) as response:
            text = response.read().decode("utf-8")
        return [json.loads(line) for line in text.splitlines() if line] if rows else text


def source_references(directory, table):
    references = []
    word = re.compile(r"(?<![a-zA-Z0-9_])" + re.escape(table) + r"(?![a-zA-Z0-9_])")
    for path in sorted((directory / "src").rglob("*.rs")):
        for line, text in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if word.search(text):
                upper = text.upper()
                role = "writer" if "INSERT INTO" in upper else "reader" if any(w in upper for w in ("SELECT ", "FROM ", "JOIN ")) else "reference"
                references.append({"path": path.relative_to(ROOT).as_posix(), "line": line,
                                   "role_hint": role, "text": text.strip()})
    return references


def inventory(client):
    report = {"scope": "fresh databases materialized from checked-in legacy DDL, not live data",
              "reference_status": "source references are audit leads; runtime ownership still requires verification",
              "chains": {}}
    for chain, (folder, explicit) in CHAINS.items():
        directory = ROOT / folder
        paths = ([directory / p for p in explicit] if explicit else
                 sorted((directory / ("sql/migrations" if chain == "bsc" else "sql")).glob("*.sql")))
        database = "aml_inventory_" + chain
        existing = client.execute(f"SELECT count() AS n FROM system.tables WHERE database='{database}'", rows=True)
        if int(existing[0]["n"]):
            raise ValueError(f"{database} is not empty; use a new disposable ClickHouse, never delete an existing database to run this tool")
        client.execute(f"CREATE DATABASE IF NOT EXISTS {database}")
        manifest = []
        for path in paths:
            raw = path.read_text(encoding="utf-8").replace("\r\n", "\n")
            rendered = raw.replace("{{database}}", database)
            if chain == "tron":
                rendered = rendered.replace("tron_db", database)
            for index, sql in enumerate(statements(rendered), 1):
                try:
                    client.execute(sql, database=database)
                except Exception as error:
                    raise RuntimeError(f"Failed at {path.relative_to(ROOT)} statement {index}") from error
            manifest.append({"path": path.relative_to(ROOT).as_posix(),
                             "normalized_sha256": hashlib.sha256(raw.encode()).hexdigest()})
        objects = client.execute("SELECT name,engine,engine_full,partition_key,sorting_key,primary_key,create_table_query,as_select FROM system.tables "
            f"WHERE database='{database}' ORDER BY name", rows=True)
        columns = client.execute("SELECT table,name,type,position,default_kind,default_expression,compression_codec,comment FROM system.columns "
            f"WHERE database='{database}' ORDER BY table,position", rows=True)
        for obj in objects:
            obj["columns"] = [c for c in columns if c["table"] == obj["name"]]
            obj["source_references"] = source_references(directory, obj["name"])
            obj["data_status"] = "no production data; seeded DDL reference data only"
        report["chains"][chain] = {"database": database, "ddl_manifest": manifest, "objects": objects}
        print(f"{chain}: {len(objects)} objects, {len(columns)} columns")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--disposable", action="store_true", help="acknowledge this creates isolated fixture databases")
    parser.add_argument("--user", default="warehouse_test")
    parser.add_argument("--password", default="warehouse-test-only", help="test credentials only; do not use production credentials")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/warehouse/legacy-inventory.json")
    args = parser.parse_args()
    if not args.disposable:
        parser.error("--disposable is required; never run against a deployed ClickHouse")
    result = inventory(TestClickHouse(args.url, args.user, args.password))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
