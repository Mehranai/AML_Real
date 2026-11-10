import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "scripts" / "prepare_ofac_sdn.py"
SOURCE_URL = "https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN.XML"
XML = """<?xml version="1.0" encoding="UTF-8"?>
<sdnList>
  <sdnEntry><uid>101</uid><lastName>Bitcoin Person</lastName><sdnType>Individual</sdnType><idList>
    <id><idType>Digital Currency Address - XBT</idType><idNumber>1BoatSLRHtKNngkdXEeobR76b53LETtpyT</idNumber></id>
  </idList></sdnEntry>
  <sdnEntry><uid>102</uid><lastName>Ethereum Entity</lastName><sdnType>Entity</sdnType><idList>
    <id><idType>Digital Currency Address - ETH</idType><idNumber>0xAbCdEf0123456789aBCdEf0123456789ABCdEf01</idNumber></id>
  </idList></sdnEntry>
  <sdnEntry><uid>103</uid><lastName>BSC Entity</lastName><sdnType>Entity</sdnType><idList>
    <id><idType>Digital Currency Address - BNB</idType><idNumber>0x1111111111111111111111111111111111111111</idNumber></id>
  </idList></sdnEntry>
  <sdnEntry><uid>104</uid><lastName>TRON Entity</lastName><sdnType>Entity</sdnType><idList>
    <id><idType>Digital Currency Address - TRX</idType><idNumber>TAmSperixPEYyEVozADE6unuKuK44QSood</idNumber></id>
    <id><idType>Digital Currency Address - LTC</idType><idNumber>LTC-not-supported</idNumber></id>
  </idList></sdnEntry>
</sdnList>
"""


class PrepareOfacSdnTests(unittest.TestCase):
    def run_tool(self, xml=XML, source_url=SOURCE_URL, expected_sha256=None, previous=None, source_path=None):
        temporary_root = ROOT / "tmp"
        temporary_root.mkdir(exist_ok=True)
        temp = tempfile.TemporaryDirectory(dir=temporary_root)
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        source = Path(source_path) if source_path is not None else base / "sdn.xml"
        output = base / "out"
        if source_path is None:
            source.write_text(xml, encoding="utf-8", newline="\n")
        checksum_args = ["--expected-sha256", expected_sha256] if expected_sha256 else []
        if previous is not None:
            previous_path = base / "previous.xml"
            previous_path.write_bytes(previous.encode())
            checksum_args += ["--previous-input", str(previous_path),
                              "--previous-sha256", hashlib.sha256(previous.encode()).hexdigest()]
        result = subprocess.run(
            [
                sys.executable,
                str(TOOL),
                "--input",
                str(source),
                "--output-dir",
                str(output),
                "--source-url",
                source_url,
                "--operator",
                "test-operator",
                "--license",
                "fixture use",
                "--observed-at",
                "2026-11-08T00:00:00Z",
                *checksum_args,
            ],
            capture_output=True,
            text=True,
            check=False,
            cwd=ROOT,
        )
        return source, output, result

    @unittest.skipUnless(os.getenv("AML_OFAC_REFERENCE_XML"), "requires the pinned official SDN XML")
    def test_reference_snapshot_preserves_public_network_attribution(self):
        # The explicit BSC/ETH entries are UID 42498 in the archived feed.
        # UID 58258 is also published in the OFAC action of 2026-08-07.
        digest = "bbf4658b7803ed6aa25712670bd6dfea56b6f9f93001d99e5d8ea221b237a597"
        _, output, result = self.run_tool(expected_sha256=digest,
                                          source_path=os.environ["AML_OFAC_REFERENCE_XML"])
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["snapshot_sha256"], digest)
        self.assertEqual(manifest["status"], "pending_review_only")

        def records(pattern):
            return [json.loads(line) for path in output.glob(pattern)
                    for line in path.read_text(encoding="utf-8").splitlines()]

        with (output / "ethereum-labels.csv").open(encoding="utf-8", newline="") as handle:
            ethereum = list(csv.DictReader(handle))
        expected = [
            (records("bitcoin-claims*.jsonl"), "1G1V1b9s9uPsFCNL15ibZk44xsVygKsP2u", 58258),
            (ethereum, "0xbb69e01921b17cd22080968bcc96ba6115da6062", 58258),
            (ethereum, "0x4f47bc496083c727c5fbe3ce9cdf2b0f6496270c", 42498),
            (records("bsc-claims.jsonl"), "0x4f47bc496083c727c5fbe3ce9cdf2b0f6496270c", 42498),
            (records("tron-labels.jsonl"), "TTVJuXWCusAURrpNShiypauagEH1N4CrxV", 58258),
        ]
        for rows, address, uid in expected:
            with self.subTest(address=address, uid=uid):
                selected = [row for row in rows if row["address"] == address]
                self.assertEqual(len(selected), 1)
                row = selected[0]
                self.assertIn(f"ofac:sdn:{uid}", row["evidence_refs"])
                self.assertIn(f"sha256:{digest}", row["evidence_refs"])
                if "review_status" in row:
                    self.assertEqual(row["review_status"].lower(), "pending")

        bsc = {row["address"] for row in records("bsc-claims.jsonl")}
        self.assertNotIn("0xbb69e01921b17cd22080968bcc96ba6115da6062", bsc)
        beacon = "bnb136ns6lfw4zs5hg4n85vdthaad7hq5m4gtkgf23"
        self.assertNotIn(beacon, bsc)
        self.assertIn(beacon, {row["address"] for row in records("excluded-addresses.jsonl")})

    def test_generates_four_pending_adapter_inputs_with_snapshot_provenance(self):
        source, output, result = self.run_tool()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["status"], "pending_review_only")
        self.assertEqual(manifest["counts"], {"bitcoin": 1, "ethereum": 1, "bsc": 1, "tron": 1})
        self.assertEqual(manifest["snapshot_sha256"], hashlib.sha256(source.read_bytes()).hexdigest())

        bitcoin = json.loads((output / "bitcoin-claims.jsonl").read_text(encoding="utf-8"))
        bsc = json.loads((output / "bsc-claims.jsonl").read_text(encoding="utf-8"))
        tron = json.loads((output / "tron-labels.jsonl").read_text(encoding="utf-8"))
        with (output / "ethereum-labels.csv").open(encoding="utf-8", newline="") as handle:
            ethereum = list(csv.DictReader(handle))
        self.assertTrue(bitcoin["is_exposure_seed"])
        self.assertEqual(bsc["review_status"], "pending")
        self.assertEqual(tron["review_status"], "PENDING")
        self.assertEqual(ethereum[0]["review_status"], "PENDING")
        self.assertEqual(ethereum[0]["address"], "0xabcdef0123456789abcdef0123456789abcdef01")
        for record in (bitcoin, bsc, tron):
            self.assertIn("sha256:", " ".join(record["evidence_refs"]))
        bsc_source = json.loads((output / "bsc-source.json").read_text(encoding="utf-8"))
        self.assertEqual(json.loads(bsc_source["metadata_json"])["snapshot_sha256"], manifest["snapshot_sha256"])

    def test_rejects_non_official_or_wrong_checksum_without_output(self):
        _, output, result = self.run_tool(source_url="https://example.invalid/sdn.xml")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("official OFAC/Treasury", result.stderr)
        self.assertFalse(output.exists())

        _, output, result = self.run_tool(expected_sha256="0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match", result.stderr)
        self.assertFalse(output.exists())

    def test_excludes_ambiguous_addresses_and_retains_other_claims(self):
        malformed = XML.replace("TAmSperixPEYyEVozADE6unuKuK44QSood", "not-a-tron-address")
        malformed = malformed.replace("0x1111111111111111111111111111111111111111", "bnb1legacy")
        _, output, result = self.run_tool(xml=malformed)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["counts"], {"bitcoin": 1, "ethereum": 1, "bsc": 0, "tron": 0})
        self.assertEqual(manifest["excluded_count"], 2)
        excluded = (output / "excluded-addresses.jsonl").read_text(encoding="utf-8").splitlines()
        self.assertEqual({json.loads(row)["symbol"] for row in excluded}, {"BNB", "TRX"})

    def test_same_address_can_be_documented_under_multiple_uids(self):
        entry = XML.split("<sdnEntry>")[1].split("</sdnEntry>")[0].replace("<uid>101", "<uid>105")
        xml = XML.replace("</sdnList>", f"<sdnEntry>{entry}</sdnEntry></sdnList>")
        _, output, result = self.run_tool(xml=xml)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = (output / "bitcoin-claims.jsonl").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len({json.loads(row)["claim_id"] for row in rows}), 2)

    def test_bitcoin_batches_fit_the_adapter_limit(self):
        entry = XML.split("<sdnEntry>")[1].split("</sdnEntry>")[0]
        new_entries = "".join(
            f"<sdnEntry>{entry.replace('<uid>101', f'<uid>{uid}')}</sdnEntry>"
            for uid in range(201, 301)
        )
        xml = XML.replace("</sdnList>", new_entries + "</sdnList>")
        _, output, result = self.run_tool(xml=xml, expected_sha256=hashlib.sha256(xml.encode()).hexdigest())
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
        self.assertTrue(manifest["expected_sha256_matched"])
        batches = sorted(output.glob("bitcoin-claims-*.jsonl"))
        self.assertEqual([len(batch.read_text(encoding="utf-8").splitlines()) for batch in batches], [100, 1])

    def test_incomplete_publication_is_rejected(self):
        xml = XML.replace(
            "<sdnList>",
            "<sdnList><publshInformation><Publish_Date>11/06/2026</Publish_Date>"
            "<Record_Count>5</Record_Count></publshInformation>",
        )
        _, output, result = self.run_tool(xml=xml)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("entry count", result.stderr)
        self.assertFalse(output.exists())

    @staticmethod
    def publication(xml, day):
        header = (f"<publshInformation><Publish_Date>11/{day}/2026</Publish_Date>"
                  f"<Record_Count>{xml.count('<sdnEntry>')}</Record_Count></publshInformation>")
        return xml.replace("<sdnList>", "<sdnList>" + header)

    def test_comparison_routes_edits_and_removals_to_review_only(self):
        entries = XML.split("<sdnEntry>")
        previous = self.publication(XML, "06")
        added = entries[1].split("</sdnEntry>")[0].replace("<uid>101", "<uid>105")
        current = XML.replace("Ethereum Entity", "Renamed Ethereum Entity")
        current = current.replace("<sdnEntry>" + entries[1], "")
        current = current.replace("TAmSperixPEYyEVozADE6unuKuK44QSood", "unreadable")
        current = current.replace("</sdnList>", f"<sdnEntry>{added}</sdnEntry></sdnList>")
        current = self.publication(current, "07")
        _, output, result = self.run_tool(current, previous=previous,
            expected_sha256=hashlib.sha256(current.encode()).hexdigest())
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["comparison"]["changes"],
                         {"added": 1, "changed": 1, "removed": 1, "unresolved": 1})
        self.assertEqual(manifest["counts"], {"bitcoin": 1, "ethereum": 0, "bsc": 0, "tron": 0})
        records = {item["change"]: item for item in map(json.loads,
                   (output / "review-delta.jsonl").read_text(encoding="utf-8").splitlines())}
        self.assertEqual(records["changed"]["before"]["claim_id"], records["changed"]["after"]["claim_id"])
        self.assertIsNone(records["removed"]["after"])
        self.assertEqual(records["unresolved"]["before"]["network"], "tron")
        self.assertIn(f"sha256:{manifest['comparison']['previous_sha256']}", records["removed"]["before"]["evidence_refs"])
        self.assertIn("ofac:sdn:105", (output / "bitcoin-claims.jsonl").read_text(encoding="utf-8"))

    def test_comparison_rejects_older_or_unverified_snapshot(self):
        previous = self.publication(XML, "07")
        current = self.publication(XML, "06")
        _, output, result = self.run_tool(current, previous=previous)
        self.assertIn("requires both expected", result.stderr)
        self.assertFalse(output.exists())
        _, output, result = self.run_tool(current, previous=previous,
            expected_sha256=hashlib.sha256(current.encode()).hexdigest())
        self.assertIn("later publication date", result.stderr)
        self.assertFalse(output.exists())
        _, output, result = self.run_tool(previous, previous=previous,
            expected_sha256=hashlib.sha256(previous.encode()).hexdigest())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((output / "review-delta.jsonl").read_bytes(), b"")


if __name__ == "__main__":
    unittest.main()
