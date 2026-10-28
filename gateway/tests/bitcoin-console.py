"""Smoke-test the real Bitcoin console in an installed Chromium/Edge, without npm.

python gateway/tests/bitcoin-console.py --browser /path/to/chromium
Uses a local fixture server and an isolated browser profile; no blockchain services.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[2]
ADDRESS = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT"
TARGET = "1BitcoinEaterAddressDontSendf59kuE"
TX = "tx:" + "a" * 64
PAYLOAD = {
    "address": ADDRESS,
    "network_id": "bip122:000000000019d6689c085ae165831e93",
    "investigation": {"id": "fixture", "network": "bitcoin", "network_id": "bitcoin mainnet",
                      "state": "temporary", "expires_at_unix_ms": 1900000000000},
    "graph": {"nodes": [{"address": ADDRESS, "node_type": "address"},
                        {"address": TX, "node_type": "transaction"},
                        {"address": TARGET, "node_type": "address"}],
              "edges": [{"id": "in:0", "from_address": ADDRESS, "to_address": TX,
                         "transfer_type": "utxo_input", "amount": "2100000000000000", "tx_hash": "a" * 64},
                        {"id": "out:0", "from_address": TX, "to_address": TARGET,
                         "transfer_type": "utxo_output", "amount": "2099999999999999", "tx_hash": "a" * 64}]},
    "holdings": {"balance_decimal": "20999999.99999999", "utxo_count": 1,
                 "history_complete": False, "as_of_block": 900000},
    "fingerprint": {"transaction_count": 2},
    "risk_engine": {"risk_score": None, "risk_level": "UNKNOWN", "policy_version": "fixture",
                    "signals": [], "limitations": ["Reviewed Bitcoin risk evidence is unavailable."]},
}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        route = self.path.split("?", 1)[0]
        files = {"/networks/bitcoin/": ROOT / "dockerizd_bitcoin/web/index.html",
                 "/assets/bitcoin.js": ROOT / "dockerizd_bitcoin/web/bitcoin.js",
                 "/assets/investigations.js": ROOT / "gateway/web/assets/investigations.js"}
        if route in files:
            body = files[route].read_bytes()
            content = "text/javascript" if route.endswith(".js") else "text/html; charset=utf-8"
        elif route == "/api/investigations":
            body = b'{"investigations":[]}'
            content = "application/json"
        elif route == f"/api/bitcoin/wallet/{ADDRESS}/investigation":
            body = json.dumps(PAYLOAD).encode()
            content = "application/json"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", content)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--browser", required=True)
    parser.add_argument("--typed",action="store_true",help="Exercise typed transaction/script nodes with null address fields")
    parser.add_argument("--stale",action="store_true",help="Exercise the historical-source warning")
    parser.add_argument("--intelligence",type=Path,help="Render actual synthetic Bitcoin/central acceptance attribution and risk")
    args = parser.parse_args()
    if args.intelligence:
        proof=json.loads(args.intelligence.read_text(encoding="utf-8"))["revoked" if args.stale else "approved"]
        for field in ("entities","intelligence","intelligence_snapshot","risk_engine"):
            PAYLOAD[field]=proof[field]
    if args.stale:
        PAYLOAD["source_validity"]={"status":"stale","current_risk_usable":False}
    if args.typed:
        kinds={node["address"]:node["node_type"] for node in PAYLOAD["graph"]["nodes"]}
        for node in PAYLOAD["graph"]["nodes"]:
            identity=node["address"];kind=node["node_type"]
            node.update(id=identity,subject_id=identity,subject_type=kind,address=identity if kind=="address" else None)
        for edge in PAYLOAD["graph"]["edges"]:
            source,target=edge["from_address"],edge["to_address"]
            edge.update(from_subject_id=source,to_subject_id=target,from_subject_type=kinds[source],to_subject_type=kinds[target],
                from_address=source if kinds[source]=="address" else None,to_address=target if kinds[target]=="address" else None)
        script="script:"+"c"*64
        PAYLOAD["graph"]["nodes"].append({"id":script,"subject_id":script,"subject_type":"script","node_type":"script","address":None})
        PAYLOAD["graph"]["edges"].append({"id":"out:1","from_subject_id":TX,"to_subject_id":script,"from_subject_type":"transaction","to_subject_type":"script","from_address":None,"to_address":None,"transfer_type":"utxo_output","amount":"0","tx_hash":"a"*64,"output_index":1})
    output = ROOT / "test-results/bitcoin-console"
    output.mkdir(parents=True, exist_ok=True)
    # Reuse no personal browser data, cookies or saved investigations.
    import tempfile
    with tempfile.TemporaryDirectory(dir=output,ignore_cleanup_errors=True) as profile:
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            url = f"http://127.0.0.1:{server.server_port}/networks/bitcoin/?address={ADDRESS}"
            flags=["--no-sandbox"] if hasattr(os,"geteuid") and os.geteuid()==0 else []
            result = subprocess.run([args.browser,*flags,"--headless=new", "--disable-gpu",
                "--no-first-run", "--disable-background-networking", "--disable-extensions",
                f"--user-data-dir={profile}", "--window-size=1440,1100", "--virtual-time-budget=5000",
                f"--screenshot={output / 'console.png'}", "--dump-dom", url],
                capture_output=True, timeout=60, check=True)
            dom = result.stdout.decode("utf-8", errors="replace")
            if not dom.strip():raise RuntimeError("Browser returned no headless DOM; use a working headless Chromium executable.")
            (output / "browser.stderr.txt").write_text(result.stderr.decode("utf-8",errors="replace"),encoding="utf-8")
            (output / "console.html").write_text(dom, encoding="utf-8")
            assert '<strong id="balance">20999999.99999999</strong>' in dom
            assert "21000000.00000000" in dom and "20999999.99999999" in dom
            assert 'id="snapshot-export"' in dom and 'id="graph"' in dom
            assert "Partial history: observed UTXOs only" in dom
            if args.intelligence and not args.stale:
                assert "APPROVED_RISK_LABEL" in dom and "70 / 100" in dom
                assert "fixture-source" in dom and "fixture-reviewer" in dom and "approved / active" in dom
            else:
                assert "Insufficient evidence" in dom and "UNKNOWN" in dom
            assert ("4 nodes, 3 edges." if args.typed else "3 nodes, 2 edges.") in dom
            if args.typed:assert "script:" in dom and "script:cccc" in dom
            if args.stale:assert "Historical snapshot: source changed or unverified" in dom
            print("PASS Bitcoin browser console: loaded evidence, exact BTC values, graph, partial history, central snapshot and unknown risk.")
            print(f"Screenshot: {output / 'console.png'}")
        finally:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    main()
