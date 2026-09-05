from __future__ import annotations

import argparse
import csv
import json
import math
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

FEATURE_NAMES = [
    "total_transfers_log",
    "unique_transactions_log",
    "incoming_transfers_log",
    "outgoing_transfers_log",
    "unique_senders_log",
    "unique_receivers_log",
    "fan_in_score",
    "fan_out_score",
    "flow_imbalance_score",
    "burst_score",
    "swap_ratio",
    "bridge_ratio",
    "exchange_interaction_ratio",
    "contract_call_ratio",
    "counterparty_concentration",
    "token_diversity_score",
    "exposure_score",
    "exposure_source_count_score",
    "exposure_path_count_score",
    "exposure_min_hop_score",
    "identity_confidence",
    "exchange_service_wallet_score",
    "truncated_sample_score",
    "data_volume_score",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build a PyTorch training CSV from labeled wallet addresses by calling the Rust TRON API."
    )
    parser.add_argument(
        "--labels",
        required=True,
        help="Input CSV with at least address,label columns.",
    )
    parser.add_argument(
        "--output",
        default="ml/tron_wallet_risk/training.csv",
        help="Output trainer-ready CSV.",
    )
    parser.add_argument(
        "--failed-output",
        default="ml/tron_wallet_risk/training_failed.csv",
        help="CSV for wallets that could not be featurized.",
    )
    parser.add_argument(
        "--labels-sql-output",
        default="",
        help="Optional SQL file to insert labels into tron_db.wallet_ml_labels.",
    )
    parser.add_argument("--api-base", default="http://127.0.0.1:4001")
    parser.add_argument("--window-days", type=int, default=90)
    parser.add_argument("--top-counterparties", type=int, default=25)
    parser.add_argument("--max-events", type=int, default=20000)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--retries", type=int, default=2)
    parser.add_argument("--sleep-ms", type=int, default=0)
    parser.add_argument("--limit", type=int, default=0, help="Optional max rows for a dry run.")
    return parser.parse_args()


def read_labels(path: Path, limit: int) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    seen_addresses: dict[str, str] = {}
    with path.open("r", newline="", encoding="utf-8-sig") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames is None:
            raise ValueError("labels CSV has no header row")
        required = {"address", "label"}
        missing = required.difference(reader.fieldnames)
        if missing:
            raise ValueError(f"labels CSV is missing required columns: {sorted(missing)}")

        for row in reader:
            address = (row.get("address") or "").strip()
            label = normalize_label(row.get("label") or "")
            if not address:
                continue
            normalized_address = address.lower()
            if normalized_address in seen_addresses:
                raise ValueError(
                    f"duplicate wallet address {address!r}; one wallet must produce one training sample"
                )
            seen_addresses[normalized_address] = label
            rows.append({"address": address, "label": label})
            if limit and len(rows) >= limit:
                break

    if not rows:
        raise ValueError("labels CSV did not contain any usable rows")
    return rows


def normalize_label(value: str) -> str:
    normalized = value.strip().lower()
    if normalized in {"1", "true", "suspicious", "illicit", "laundering", "ml"}:
        return "1"
    if normalized in {"0", "false", "normal", "clean", "benign"}:
        return "0"
    raise ValueError(f"unsupported label value: {value!r}")


def wallet_ai_risk_url(args: argparse.Namespace, address: str) -> str:
    query = urllib.parse.urlencode(
        {
            "window_days": str(args.window_days),
            "top_counterparties": str(args.top_counterparties),
            "max_events": str(args.max_events),
        }
    )
    encoded_address = urllib.parse.quote(address, safe="")
    return f"{args.api_base.rstrip('/')}/api/tron/wallet/{encoded_address}/ai-risk?{query}"

def fetch_wallet_features(args: argparse.Namespace, address: str) -> dict[str, str | float | int]:
    url = wallet_ai_risk_url(args, address)
    last_error: Exception | None = None

    for attempt in range(args.retries + 1):
        try:
            request = urllib.request.Request(url, headers={"accept": "application/json"})
            with urllib.request.urlopen(request, timeout=args.timeout) as response:
                payload = json.loads(response.read().decode("utf-8"))
            snapshot = payload.get("feature_snapshot", {})
            features = snapshot.get("features", {})
            if not isinstance(features, dict):
                raise ValueError("API response did not include feature_snapshot.features")
            missing = [feature for feature in FEATURE_NAMES if feature not in features]
            if missing:
                raise ValueError(f"API response is missing required features: {missing}")

            result = {feature: float(features[feature]) for feature in FEATURE_NAMES}
            non_finite = [
                feature for feature, value in result.items() if not math.isfinite(value)
            ]
            if non_finite:
                raise ValueError(f"API response contains non-finite features: {non_finite}")
            snapshot_id = str(snapshot.get("snapshot_id") or "").strip()
            schema_version = str(snapshot.get("feature_schema_version") or "").strip()
            generated_at = int(snapshot.get("generated_at_unix_ms") or 0)
            if not snapshot_id or not schema_version or generated_at <= 0:
                raise ValueError("API response has incomplete feature snapshot provenance")
            return {
                "snapshot_id": snapshot_id,
                "feature_schema_version": schema_version,
                "feature_generated_at_unix_ms": generated_at,
                **result,
            }
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, ValueError) as exc:
            last_error = exc
            if attempt < args.retries:
                time.sleep(1.0 + attempt)

    raise RuntimeError(f"failed to build features for {address}: {last_error}")


def write_label_sql(path: Path, labels: list[dict[str, str]]) -> None:
    now_ms = int(time.time() * 1000)
    values = []
    for row in labels:
        label = int(row["label"])
        label_name = "money_laundering" if label == 1 else "normal_clean"
        values.append(
            "("
            f"{sql_string(row['address'])}, "
            f"{label}, "
            f"{sql_string(label_name)}, "
            "[], "
            "1.0, "
            "'manual_labeled_wallet_csv', "
            "'manual_wallet_training_set', "
            "[], "
            f"{now_ms}"
            ")"
        )

    statement = "\n".join(
        [
            "INSERT INTO tron_db.wallet_ml_labels",
            "(",
            "    address,",
            "    label,",
            "    label_name,",
            "    typologies,",
            "    confidence,",
            "    source,",
            "    case_id,",
            "    evidence_refs,",
            "    created_at_unix_ms",
            ")",
            "VALUES",
            ",\n".join(values) + ";",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(statement + "\n", encoding="utf-8")


def sql_string(value: str) -> str:
    return "'" + value.replace("\\", "\\\\").replace("'", "''") + "'"


def main() -> None:
    args = parse_args()
    labels = read_labels(Path(args.labels), args.limit)

    output_path = Path(args.output)
    failed_path = Path(args.failed_output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    failed_path.parent.mkdir(parents=True, exist_ok=True)

    if args.labels_sql_output:
        write_label_sql(Path(args.labels_sql_output), labels)

    with output_path.open("w", newline="", encoding="utf-8") as output_handle, failed_path.open(
        "w", newline="", encoding="utf-8"
    ) as failed_handle:
        writer = csv.DictWriter(
            output_handle,
            fieldnames=[
                "address",
                "label",
                "snapshot_id",
                "feature_schema_version",
                "feature_generated_at_unix_ms",
                *FEATURE_NAMES,
            ],
        )
        failed_writer = csv.DictWriter(failed_handle, fieldnames=["address", "label", "error"])
        writer.writeheader()
        failed_writer.writeheader()

        for index, row in enumerate(labels, start=1):
            address = row["address"]
            try:
                features = fetch_wallet_features(args, address)
                writer.writerow({"address": address, "label": row["label"], **features})
                print(f"[{index}/{len(labels)}] ok {address}")
            except Exception as exc:
                failed_writer.writerow(
                    {"address": address, "label": row["label"], "error": str(exc)}
                )
                print(f"[{index}/{len(labels)}] failed {address}: {exc}")

            if args.sleep_ms > 0:
                time.sleep(args.sleep_ms / 1000.0)

    print(f"wrote training CSV: {output_path}")
    print(f"wrote failed rows: {failed_path}")


if __name__ == "__main__":
    main()
