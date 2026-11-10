"""Verify existing live four-chain APIs through Gateway and real Neo4j.

No ingestion, schema changes, fact seeds, mock upstreams or database resets.
Creates and exports investigations in the supplied acceptance environment.
See docs/TESTING.md for configuration and the bounded scope of this test.
"""
import argparse
import base64
import copy
from decimal import Decimal
import http.client
from http.cookiejar import CookieJar
import json
import os
from pathlib import Path
import time
import urllib.error
import urllib.request

NETWORKS = ("bitcoin", "ethereum", "bsc", "tron")
MAX_BYTES = 32 * 1024 * 1024


class Client:
    def __init__(self):
        self.cookies = CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.cookies))

    def request(self, url, payload=None, headers=None, expected=200):
        headers = dict(headers or {})
        headers.setdefault("User-Agent", "aml-live-e2e/1.0")
        body = None
        if payload is not None:
            headers["Content-Type"] = "application/json"
            body = json.dumps(payload).encode()
        request = urllib.request.Request(url, data=body, headers=headers)
        try:
            response = self.opener.open(request, timeout=180)
        except urllib.error.HTTPError as error:
            response = error
        except (urllib.error.URLError, TimeoutError) as error:
            raise RuntimeError("Live HTTP request failed: " + type(error).__name__) from None
        with response:
            assert response.code == expected, f"expected HTTP {expected}, received {response.code}"
            data = response.read(MAX_BYTES + 1)
            assert len(data) <= MAX_BYTES, "HTTP response exceeds acceptance limit"
            return data, response.headers

    def json(self, url, payload=None, headers=None, expected=200):
        data, result_headers = self.request(url, payload, headers, expected)
        return json.loads(data, parse_float=Decimal), result_headers


def comparable_observation(evidence):
    value = copy.deepcopy(evidence)
    # Holdings are a second, current-RPC observation.  Two requests for the
    # same fixed transfer can land on different live EVM heads, changing both
    # its snapshot and current balance.  Compare immutable evidence here; the
    # complete, unmodified evidence is still checked exactly after reopen.
    value.pop("holdings", None)
    return value


def tron_address(raw):
    import hashlib
    data = bytes.fromhex(raw.removeprefix("0x"))
    data += hashlib.sha256(hashlib.sha256(data).digest()).digest()[:4]
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    number, result = int.from_bytes(data, "big"), ""
    while number:
        number, remainder = divmod(number, 58)
        result = alphabet[remainder] + result
    return "1" * (len(data) - len(data.lstrip(b"\0"))) + result


def rpc_reference(network, config):
    client = Client()
    endpoint = config.get("rpc_url") or os.environ[config["rpc_url_env"]]

    def rpc_once(method, params):
        if network == "tron":
            result, _ = client.json(endpoint.rstrip("/") + "/" + method, params)
            assert "Error" not in result and "error" not in result, "TRON RPC rejected reference"
            return result
        headers = {}
        if config.get("rpc_user_env"):
            pair = os.environ[config["rpc_user_env"]] + ":" + os.environ[config["rpc_password_env"]]
            headers["Authorization"] = "Basic " + base64.b64encode(pair.encode()).decode()
        result, _ = client.json(endpoint, {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}, headers)
        assert not result.get("error"), "JSON RPC rejected reference"
        return result["result"]

    def rpc(method, params):
        # Public endpoints occasionally truncate a large full-block response.
        # These are read-only reference calls, so a small bounded retry is safe.
        for attempt in range(3):
            try:
                return rpc_once(method, params)
            except (http.client.IncompleteRead, RuntimeError):
                if attempt == 2:
                    raise
                time.sleep(attempt + 1)

    height, tx_hash = config["block_number"], config["tx_hash"]
    if network == "bitcoin":
        block_hash = rpc("getblockhash", [height])
        block = rpc("getblock", [block_hash, 3])
        tx = next(t for t in block["tx"] if t["txid"] == tx_hash)
        output = tx["vout"][config["output_index"]]
        assert output["scriptPubKey"].get("address") == config["target"]
        assert any(i.get("prevout", {}).get("scriptPubKey", {}).get("address") == config["address"] for i in tx["vin"])
        sats = output["value"] * Decimal(100_000_000)
        assert sats == sats.to_integral_value()
        amount, kind = str(int(sats)), "utxo_output"
    elif network in ("ethereum", "bsc"):
        block = rpc("eth_getBlockByNumber", [hex(height), True])
        block_hash = block["hash"]
        tx = next(t for t in block["transactions"] if t["hash"].lower() == tx_hash.lower())
        receipt = rpc("eth_getTransactionReceipt", [tx_hash])
        assert receipt["blockHash"].lower() == block_hash.lower() and int(receipt["status"], 16) == 1
        assert tx["from"].lower() == config["address"].lower() and tx["to"].lower() == config["target"].lower()
        amount, kind = str(int(tx["value"], 16)), "native"
    else:
        block = rpc("wallet/getblockbynum", {"num": height})
        block_hash = block["blockID"]
        tx = next(t for t in block["transactions"] if t["txID"].lower() == tx_hash.lower())
        contract = tx["raw_data"]["contract"][0]
        assert contract["type"] == "TransferContract" and len(tx["raw_data"]["contract"]) == 1
        transfer = contract["parameter"]["value"]
        assert tron_address(transfer["owner_address"]) == config["address"]
        assert tron_address(transfer["to_address"]) == config["target"]
        receipt = rpc("wallet/gettransactioninfobyid", {"value": tx_hash})
        assert receipt["id"].lower() == tx_hash.lower() and receipt["blockNumber"] == height
        assert receipt.get("result", "SUCCESS") == "SUCCESS"
        amount, kind = str(transfer["amount"]), "native"
    assert block_hash.lower() == config["block_hash"].lower(), "RPC block identity changed"
    return {"tx_hash": tx_hash.lower(), "amount_raw": amount, "kind": kind}


def graph(value):
    assert value["response_contract"] == "chain_response_v1"
    assert value["graph"]["nodes"] and value["graph"]["edges"]
    assert value["evidence"]["source_snapshot_id"] == value["source_snapshot"]["snapshot_id"]
    for edge in value["graph"]["edges"]:
        assert isinstance(edge["amount_raw"], str) and edge["amount_raw"].isdigit()
    return value["graph"]


def run(config):
    gateway = config["gateway_url"].rstrip("/")
    assert set(config["networks"]) == set(NETWORKS), "all four networks required"
    key = os.environ[config.get("service_key_env", "AML_SERVICE_KEY")]
    browser, direct = Client(), Client()
    _, headers = browser.json(gateway + "/api/investigations")
    assert "no-store" in headers.get("Cache-Control", "")
    assert any(cookie.name == "aml_session" for cookie in browser.cookies)
    password = os.environ[config.get("neo4j_password_env", "NEO4J_PASSWORD")]
    neo_headers = {"Authorization": "Basic " + base64.b64encode(("neo4j:" + password).encode()).decode()}

    def neo(statement, params):
        value, _ = direct.json(config["neo4j_url"].rstrip("/") + "/db/neo4j/query/v2", {"statement": statement, "parameters": params}, neo_headers, 202)
        assert not value.get("errors"), "Neo4j rejected projection verification"
        return value["data"]["values"]

    results = []
    for network in NETWORKS:
        cfg = config["networks"][network]
        reference = rpc_reference(network, cfg)
        page, _ = browser.request(gateway + f"/networks/{network}/")
        assert b"investigations.js" in page, "real panel does not load central snapshot controls"
        api = cfg["api_url"].rstrip("/")
        for mode in ("wallet", "paths"):
            path = f'/api/{network}/wallet/{cfg["address"]}/' + ("investigation" if mode == "wallet" else "paths/" + cfg["target"])
            direct.json(api + path, expected=401)
            source, _ = direct.json(api + path, headers={"x-aml-service-key": key})
            expected_graph = graph(source)
            matches = [e for e in expected_graph["edges"] if e["tx_hash"].lower() == reference["tx_hash"] and e["amount_raw"] == reference["amount_raw"]]
            if network == "bitcoin":
                matches = [e for e in matches if e["transfer_type"] == "utxo_output" and e["event_index"] == cfg["output_index"]]
            else:
                asset = {"ethereum": "native:ETH", "bsc": "native:BNB", "tron": "native:TRX"}[network]
                matches = [e for e in matches if e["from_address"] == cfg["address"] and e["to_address"] == cfg["target"] and e["canonical_asset_id"] == asset]
            assert matches, f"{network}/{mode}: RPC-native amount missing from API graph"
            created, headers = browser.json(gateway + path)
            assert "no-store" in headers.get("Cache-Control", "")
            assert created["source_validity"]["status"] == "current", f"{network}/{mode}: live source unexpectedly stale"
            assert graph(created) == expected_graph
            assert comparable_observation(created["evidence"]) == comparable_observation(source["evidence"])
            identifier = created["investigation"]["id"]
            opened, _ = browser.json(gateway + "/api/investigations/" + identifier)
            assert opened["graph"] == expected_graph and opened["evidence"] == created["evidence"]
            assert opened["source_validity"]["status"] == "current"
            projected = neo("MATCH ()-[r:TRANSFER {investigation_id:$id}]->() RETURN r.edge_id,r.amount,r.asset_id,r.token_id,r.transaction_index,r.event_index,r.event_sub_index", {"id": identifier})
            assert len(projected) == len(expected_graph["edges"])
            by_id = {e["id"]: e for e in expected_graph["edges"]}
            for row in projected:
                edge = by_id[row[0]]
                for index, field in enumerate(("amount_raw", "canonical_asset_id", "canonical_token_id", "transaction_index", "event_index", "event_sub_index"), 1):
                    assert row[index] == edge.get(field), f"{network}: altered Neo4j {field}"
            assert neo("MATCH (w:InvestigationWallet {investigation_id:$id}) WHERE w.subject_type <> 'address' AND w.address IS NOT NULL RETURN count(w)", {"id": identifier}) == [[0]]
            stranger = Client()
            stranger.json(gateway + "/api/investigations/" + identifier, headers={"X-AML-User": "spoofed-owner"}, expected=404)
            stranger.json(gateway + "/api/investigations/" + identifier + "/export", {}, expected=404)
            exported, _ = browser.json(gateway + "/api/investigations/" + identifier + "/export", {})
            assert exported["investigation"]["state"] == "saved"
            saved, _ = browser.json(gateway + "/api/investigations")
            assert any(item["id"] == identifier for item in saved["investigations"])
            reopened, _ = browser.json(gateway + "/api/investigations/" + identifier)
            assert reopened["graph"] == expected_graph and reopened["evidence"] == created["evidence"]
            results.append({"network": network, "mode": mode, "id": identifier, "edges": len(projected), "rpc_amount_raw": reference["amount_raw"], "source_status": reopened["source_validity"]["status"]})
            print(f"PASS {network}/{mode}: RPC amount, live API, Gateway, Neo4j, reopen/export and owner isolation", flush=True)
    return {"status": "pass", "scope": "bounded_live_rpc_and_services_not_full_archive_or_vm_capacity", "results": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = run(json.loads(args.config.read_text(encoding="utf-8-sig")))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
