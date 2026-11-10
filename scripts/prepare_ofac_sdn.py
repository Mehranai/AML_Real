#!/usr/bin/env python3
"""Convert a locally downloaded OFAC legacy SDN XML snapshot into pending AML claims.

The tool never downloads data, writes to ClickHouse, or approves a claim.  It
keeps the source file outside Git and emits adapter-specific import files with
the snapshot SHA-256 and the OFAC entry UID in their provenance.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
import re
from urllib.parse import urlparse
import xml.etree.ElementTree as ET

MAX_INPUT_BYTES = 64 * 1024 * 1024
NETWORKS = {
    "XBT": ("bitcoin", "bip122:000000000019d6689c085ae165831e93"),
    "BTC": ("bitcoin", "bip122:000000000019d6689c085ae165831e93"),
    "ETH": ("ethereum", "eip155:1"),
    "BNB": ("bsc", "eip155:56"),
    "BSC": ("bsc", "eip155:56"),
    "TRX": ("tron", "tron:mainnet"),
}
OFFICIAL_HOSTS = {
    "ofac.treasury.gov",
    "home.treasury.gov",
    "sanctionslist.ofac.treas.gov",
    "sanctionslistservice.ofac.treas.gov",
    "www.treasury.gov",
}
ADDRESS_TYPE = re.compile(r"^Digital Currency Address\s*-\s*([A-Za-z0-9]+)$", re.I)
EVM_ADDRESS = re.compile(r"^0x[0-9a-fA-F]{40}$")
BASE58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BITCOIN_WITNESS = re.compile(r"^bc1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{7,87}$")


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def direct_text(node: ET.Element, *names: str) -> str:
    wanted = set(names)
    for child in node:
        if local_name(child.tag) in wanted and child.text:
            return child.text.strip()
    return ""


def validated_source_url(value: str) -> str:
    parsed = urlparse(value)
    if (
        len(value) > 1000
        or any(char.isspace() or char == "|" for char in value)
        or parsed.scheme != "https"
        or parsed.username is not None
        or parsed.password is not None
        or parsed.fragment
        or parsed.hostname not in OFFICIAL_HOSTS
        or parsed.port not in (None, 443)
    ):
        raise ValueError("--source-url must be an HTTPS URL on an official OFAC/Treasury host")
    return value


def read_snapshot(path: Path, expected_digest: str | None) -> tuple[str, str]:
    with path.open("rb") as handle:
        content = handle.read(MAX_INPUT_BYTES + 1)
    if not content:
        raise ValueError("OFAC snapshot is empty")
    if len(content) > MAX_INPUT_BYTES:
        raise ValueError(f"OFAC snapshot exceeds {MAX_INPUT_BYTES} bytes")
    xml = content.decode("utf-8-sig")
    if "\0" in xml or re.search(r"<!DOCTYPE|<!ENTITY", xml, re.I):
        raise ValueError("OFAC XML with DTD/entity declarations is not accepted")
    digest = hashlib.sha256(content).hexdigest()
    if expected_digest is not None and digest != expected_digest.lower():
        raise ValueError("OFAC snapshot SHA-256 does not match the expected digest")
    return xml, digest


def valid_base58check(address: str, versions: tuple[int, ...]) -> bool:
    if not 26 <= len(address) <= 35:
        return False
    number = 0
    for char in address:
        if char not in BASE58:
            return False
        number = number * 58 + BASE58.index(char)
    leading_zeros = len(address) - len(address.lstrip("1"))
    decoded = bytes(leading_zeros) + number.to_bytes((number.bit_length() + 7) // 8, "big")
    if len(decoded) != 25 or decoded[0] not in versions:
        return False
    checksum = hashlib.sha256(hashlib.sha256(decoded[:-4]).digest()).digest()[:4]
    return decoded[-4:] == checksum


def normalize_address(network: str, address: str) -> str | None:
    address = address.strip()
    if network in {"ethereum", "bsc"}:
        return address.lower() if EVM_ADDRESS.fullmatch(address) and int(address[2:], 16) else None
    if network == "tron":
        return address if valid_base58check(address, (65,)) else None
    if valid_base58check(address, (0, 5)):
        return address
    # The Bitcoin adapter verifies witness version, program and checksum on import.
    if (address.islower() or address.isupper()) and BITCOIN_WITNESS.fullmatch(address.lower()):
        return address.lower()
    return None


def entity_name(entry: ET.Element, uid: str) -> str:
    first = direct_text(entry, "firstName")
    last = direct_text(entry, "lastName", "name")
    return " ".join(filter(None, (first, last))).strip() or f"OFAC SDN {uid}"


def parse_entries(
    content: str, source_url: str, source_id: str, digest: str, *, comparison: bool = False,
) -> tuple[list[dict], list[dict], str | None]:
    try:
        root = ET.fromstring(content)
    except ET.ParseError as error:
        raise ValueError(f"invalid OFAC XML: {error}") from error
    claims: dict[str, dict] = {}
    excluded: list[dict] = []
    entries = [node for node in root if local_name(node.tag) == "sdnEntry"]
    if local_name(root.tag) != "sdnList" or not entries:
        raise ValueError("expected legacy OFAC SDN XML with one or more sdnEntry elements")
    publication = next((node for node in root if local_name(node.tag) == "publshInformation"), None)
    published_date = None
    if publication is not None:
        declared_count = direct_text(publication, "Record_Count")
        if not declared_count.isascii() or not declared_count.isdecimal() or int(declared_count) != len(entries):
            raise ValueError("SDN entry count differs from the publication header")
        published_date = datetime.strptime(direct_text(publication, "Publish_Date"), "%m/%d/%Y").date().isoformat()
    elif comparison:
        raise ValueError("snapshot comparison requires a complete publication header")
    source_key = hashlib.sha256(source_id.encode()).hexdigest()[:16]
    seen_uids: set[str] = set()
    for entry in entries:
        uid = direct_text(entry, "uid")
        if not re.fullmatch(r"[0-9]{1,20}", uid) or uid in seen_uids:
            raise ValueError("SDN entries require unique numeric UIDs")
        seen_uids.add(uid)
        name = entity_name(entry, uid)
        entity_type = direct_text(entry, "sdnType").lower() or "sanctioned_party"
        for identifier in (node for node in entry.iter() if local_name(node.tag) == "id"):
            kind = direct_text(identifier, "idType")
            match = ADDRESS_TYPE.fullmatch(kind)
            if not match:
                continue
            symbol = match.group(1).upper()
            if symbol not in NETWORKS:
                continue
            network, network_id = NETWORKS[symbol]
            source_address = direct_text(identifier, "idNumber")
            address = normalize_address(network, source_address)
            if not address:
                excluded.append({"uid": uid, "symbol": symbol, "address": source_address,
                                 "reason": "address_not_valid_for_supported_network"})
                continue
            address_hash = hashlib.sha256(address.encode()).hexdigest()[:16]
            claim_id = f"ofac-sdn:{source_key}:{uid}:{network}:{address_hash}"
            if claim_id in claims:
                claims[claim_id]["symbols"].add(symbol)
                continue
            claims[claim_id] = {
                "network": network,
                "network_id": network_id,
                "address": address,
                "claim_id": claim_id,
                "entity_id": f"ofac-sdn-{uid}",
                "entity_name": name,
                "entity_type": entity_type,
                "uid": uid,
                "symbols": {symbol},
                "source_record_id": f"ofac-sdn:{uid}:{address_hash}",
                "source_reference": f"{source_url}#uid={uid}",
                "evidence_refs": [f"ofac:sdn:{uid}", f"sha256:{digest}", source_url],
            }
    if not claims and not comparison:
        raise ValueError("snapshot contains no supported Bitcoin, Ethereum, BSC, or TRON addresses")
    for claim in claims.values():
        claim["symbols"] = sorted(claim["symbols"])
        claim["evidence_refs"][1:1] = [f"ofac:currency:{symbol}" for symbol in claim["symbols"]]
    ordered = sorted(claims.values(), key=lambda claim: (claim["network"], claim["address"], claim["claim_id"]))
    return ordered, excluded, published_date


def compare_claims(previous: list[dict], current: list[dict], excluded: list[dict]) -> list[dict]:
    old = {claim["claim_id"]: claim for claim in previous}
    new = {claim["claim_id"]: claim for claim in current}
    ambiguous = {(item["uid"], NETWORKS[item["symbol"]][0]) for item in excluded}
    changes = []
    for claim_id in sorted(old.keys() | new.keys()):
        before, after = old.get(claim_id), new.get(claim_id)
        if before is None:
            change = "added"
        elif after is None:
            change = "unresolved" if (before["uid"], before["network"]) in ambiguous else "removed"
        elif any(before[key] != after[key] for key in ("entity_name", "entity_type", "symbols")):
            change = "changed"
        else:
            continue
        changes.append({"change": change, "claim_id": claim_id, "before": before, "after": after})
    return changes


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text("".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in rows), encoding="utf-8")


def source(network_id: str, args: argparse.Namespace, digest: str, observed_at: str) -> dict:
    return {
        "network_id": network_id,
        "source_id": args.source_id,
        "source_name": "OFAC Specially Designated Nationals (SDN) List",
        "source_type": "REGULATORY",
        "trust_tier": "HIGH",
        "reference_url": args.source_url,
        "license": args.license,
        "is_active": True,
        "created_by": args.operator,
        "metadata_json": json.dumps(
            {"snapshot_sha256": digest, "observed_at": observed_at, "format": "legacy_sdn_xml"},
            separators=(",", ":"),
        ),
    }


def emit(
    output: Path, claims: list[dict], excluded: list[dict], args: argparse.Namespace,
    digest: str, observed_at: str, published_date: str | None, *,
    comparison: dict | None = None, changes: list[dict] | None = None,
) -> None:
    output.mkdir(parents=True, exist_ok=False)
    by_network = {network: [claim for claim in claims if claim["network"] == network] for network in ("bitcoin", "ethereum", "bsc", "tron")}
    manifest = {
        "format": "aml_ofac_sdn_pending_v1",
        "source_id": args.source_id,
        "source_url": args.source_url,
        "snapshot_sha256": digest,
        "expected_sha256_matched": args.expected_sha256 is not None,
        "observed_at": observed_at,
        "published_date": published_date,
        "license": args.license,
        "status": "pending_review_only",
        "counts": {network: len(rows) for network, rows in by_network.items()},
        "excluded_count": len(excluded),
    }
    if excluded:
        write_jsonl(output / "excluded-addresses.jsonl", excluded)
    if comparison is not None and changes is not None:
        manifest["comparison"] = comparison
        write_jsonl(output / "review-delta.jsonl", changes)

    bitcoin_source = source(NETWORKS["XBT"][1], args, digest, observed_at)
    bitcoin_source.pop("metadata_json")
    write_json(output / "bitcoin-source.json", bitcoin_source)
    bitcoin_claims = [
            {
                "network_id": claim["network_id"], "claim_id": claim["claim_id"], "address": claim["address"],
                "entity_id": claim["entity_id"], "entity_name": claim["entity_name"], "entity_type": claim["entity_type"],
                "address_role": "unknown", "confidence": 1.0, "risk_level": 100, "is_exposure_seed": True,
                "seed_category": "sanctions", "source_id": args.source_id, "source_reference": claim["source_reference"],
                "evidence_refs": claim["evidence_refs"], "submitted_by": args.operator,
            }
            for claim in by_network["bitcoin"]
    ]
    # bitcoin_aml limits each import to 100 claims.
    if len(bitcoin_claims) <= 100:
        write_jsonl(output / "bitcoin-claims.jsonl", bitcoin_claims)
    else:
        for start in range(0, len(bitcoin_claims), 100):
            write_jsonl(output / f"bitcoin-claims-{start // 100 + 1:03}.jsonl",
                        bitcoin_claims[start:start + 100])

    ethereum_headers = [
        "address", "entity_id", "entity_name", "entity_type", "address_role", "confidence", "risk_level",
        "is_exposure_seed", "seed_category", "source_record_id", "evidence_refs", "review_status", "reviewed_by", "review_reason",
    ]
    with (output / "ethereum-labels.csv").open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=ethereum_headers)
        writer.writeheader()
        for claim in by_network["ethereum"]:
            writer.writerow({
                "address": claim["address"], "entity_id": claim["entity_id"], "entity_name": claim["entity_name"],
                "entity_type": claim["entity_type"], "address_role": "UNKNOWN", "confidence": "1.0", "risk_level": "100",
                "is_exposure_seed": "true", "seed_category": "SANCTIONS", "source_record_id": claim["source_record_id"],
                "evidence_refs": "|".join(claim["evidence_refs"]), "review_status": "PENDING", "reviewed_by": "", "review_reason": "",
            })

    write_json(output / "bsc-source.json", source(NETWORKS["BNB"][1], args, digest, observed_at))
    write_jsonl(
        output / "bsc-claims.jsonl",
        [
            {
                "network_id": claim["network_id"], "claim_id": claim["claim_id"], "address": claim["address"],
                "claim_kind": "label", "entity_id": claim["entity_id"], "entity_name": claim["entity_name"],
                "entity_type": claim["entity_type"], "address_role": "unknown", "confidence": 1.0, "risk_level": 100,
                "is_exposure_seed": True, "seed_category": "sanctions", "source_id": args.source_id,
                "source_reference": claim["source_reference"], "evidence_refs": claim["evidence_refs"],
                "review_status": "pending", "reviewed_by": "", "review_note": "",
            }
            for claim in by_network["bsc"]
        ],
    )

    tron_source = source(NETWORKS["TRX"][1], args, digest, observed_at)
    tron_source.pop("metadata_json")
    tron_source.pop("network_id")
    write_json(output / "tron-source.json", tron_source)
    write_jsonl(
        output / "tron-labels.jsonl",
        [
            {
                "address": claim["address"], "label_id": claim["claim_id"], "entity_id": claim["entity_id"],
                "entity_name": claim["entity_name"], "entity_type": claim["entity_type"], "address_role": "UNKNOWN",
                "confidence": 1.0, "risk_percent": 100, "source": args.source_id,
                "source_record_id": claim["source_record_id"], "submitted_by": args.operator,
                "evidence_refs": claim["evidence_refs"], "review_status": "PENDING", "reviewed_by": "", "review_reason": "",
            }
            for claim in by_network["tron"]
        ],
    )
    write_json(output / "manifest.json", manifest)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True, help="locally downloaded legacy OFAC SDN XML")
    parser.add_argument("--output-dir", type=Path, required=True, help="private directory for generated pending imports")
    parser.add_argument("--source-url", required=True, help="exact official OFAC/Treasury download or list URL")
    parser.add_argument("--operator", required=True, help="operator recorded as source creator/submission actor")
    parser.add_argument("--source-id", default="ofac-sdn", help="stable source id used across snapshots")
    parser.add_argument("--license", required=True, help="permitted-use record retained with the source")
    parser.add_argument("--expected-sha256", help="compare the bytes with the SHA-256 published by OFAC")
    parser.add_argument("--previous-input", type=Path, help="previous complete SDN XML for comparison")
    parser.add_argument("--previous-sha256", help="expected SHA-256 for the previous file")
    parser.add_argument("--observed-at", help="RFC 3339 observation timestamp; defaults to current UTC time")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", args.source_id):
        parser.error("--source-id must be 1..128 letters, digits, underscore or hyphen")
    for digest_arg in (args.expected_sha256, args.previous_sha256):
        if digest_arg is not None and not re.fullmatch(r"[A-Fa-f0-9]{64}", digest_arg):
            parser.error("expected SHA-256 values must contain 64 hexadecimal digits")
    if args.previous_input is not None and not (args.expected_sha256 and args.previous_sha256):
        parser.error("snapshot comparison requires both expected SHA-256 values")
    if args.previous_input is None and args.previous_sha256 is not None:
        parser.error("--previous-sha256 requires --previous-input")
    for value in (args.operator, args.license):
        if not value.strip() or len(value.encode()) > 2000 or any(ord(char) < 32 for char in value):
            parser.error("operator and license must be non-empty text without control characters")
    try:
        args.source_url = validated_source_url(args.source_url)
        content, digest = read_snapshot(args.input, args.expected_sha256)
        observed_at = datetime.fromisoformat(args.observed_at.replace("Z", "+00:00")) if args.observed_at else datetime.now(timezone.utc)
        if observed_at.utcoffset() is None:
            raise ValueError("--observed-at needs a UTC offset")
        claims, excluded, published_date = parse_entries(
            content, args.source_url, args.source_id, digest, comparison=args.previous_input is not None,
        )
        comparison = changes = None
        if args.previous_input is not None:
            old_content, old_digest = read_snapshot(args.previous_input, args.previous_sha256)
            old_claims, _, old_date = parse_entries(
                old_content, args.source_url, args.source_id, old_digest, comparison=True,
            )
            if digest != old_digest and published_date <= old_date:
                raise ValueError("the new snapshot must have a later publication date")
            changes = compare_claims(old_claims, claims, excluded)
            comparison = {
                "previous_sha256": old_digest,
                "previous_published_date": old_date,
                "changes": {kind: sum(item["change"] == kind for item in changes)
                            for kind in ("added", "changed", "removed", "unresolved")},
                "import_scope": "additions_only",
            }
            claims = [item["after"] for item in changes if item["change"] == "added"]
        emit(args.output_dir, claims, excluded, args, digest,
             observed_at.astimezone(timezone.utc).isoformat(), published_date,
             comparison=comparison, changes=changes)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps({"status": "pending_review_only", "snapshot_sha256": digest,
                      "claims": len(claims), "excluded": len(excluded)}))


if __name__ == "__main__":
    main()
