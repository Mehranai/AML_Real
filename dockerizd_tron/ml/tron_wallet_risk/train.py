
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import random
import time
import uuid
from dataclasses import dataclass
from pathlib import Path

import torch
from torch import nn
from torch.utils.data import DataLoader, TensorDataset


FEATURE_SCHEMA_VERSION = "tron_wallet_behavior_features_v2"

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


@dataclass
class Dataset:
    addresses: list[str]
    features: torch.Tensor
    labels: torch.Tensor


class WalletRiskMlp(nn.Module):
    def __init__(self, input_width: int, hidden_widths: list[int]) -> None:
        super().__init__()
        self.hidden_layers = nn.ModuleList()
        previous_width = input_width
        for width in hidden_widths:
            self.hidden_layers.append(nn.Linear(previous_width, width))
            previous_width = width
        self.output_layer = nn.Linear(previous_width, 1)

    def forward(self, inputs: torch.Tensor) -> torch.Tensor:
        values = inputs
        for layer in self.hidden_layers:
            values = torch.relu(layer(values))
        return self.output_layer(values).squeeze(-1)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Train a PyTorch MLP for TRON wallet AML risk scoring."
    )
    parser.add_argument("--input", required=True, help="Training CSV path.")
    parser.add_argument(
        "--output-dir",
        default="ml/tron_wallet_risk/artifacts/latest",
        help="Directory for exported artifact files.",
    )
    parser.add_argument("--model-id", default="tron_wallet_pytorch_mlp_v1")
    parser.add_argument("--model-version", default="v1")
    parser.add_argument("--dataset-id", default="manual_csv_v1")
    parser.add_argument(
        "--label-policy",
        default="label_1_laundering_label_0_benign",
        help="Human readable label policy saved with the model.",
    )
    parser.add_argument(
        "--hidden-widths",
        default="32,16",
        help="Comma-separated hidden layer widths.",
    )
    parser.add_argument("--epochs", type=int, default=200)
    parser.add_argument("--batch-size", type=int, default=64)
    parser.add_argument("--learning-rate", type=float, default=1e-3)
    parser.add_argument("--weight-decay", type=float, default=1e-4)
    parser.add_argument("--validation-ratio", type=float, default=0.15)
    parser.add_argument("--test-ratio", type=float, default=0.15)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--minimum-test-samples", type=int, default=200)
    parser.add_argument("--minimum-test-auc", type=float, default=0.70)
    parser.add_argument("--maximum-test-brier", type=float, default=0.25)
    parser.add_argument(
        "--activate",
        action="store_true",
        help="Write model registry SQL with status ACTIVE instead of CANDIDATE.",
    )
    return parser.parse_args()


def parse_hidden_widths(value: str) -> list[int]:
    widths = [int(item.strip()) for item in value.split(",") if item.strip()]
    if not widths:
        raise ValueError("at least one hidden width is required")
    if any(width <= 0 for width in widths):
        raise ValueError("hidden widths must be positive")
    return widths


def parse_label(value: str) -> float:
    normalized = value.strip().lower()
    if normalized in {"1", "true", "illicit", "laundering", "ml", "suspicious"}:
        return 1.0
    if normalized in {"0", "-1", "false", "benign", "clean", "normal"}:
        return 0.0
    raise ValueError(f"unsupported label value: {value!r}")


def read_training_csv(path: Path) -> Dataset:
    addresses: list[str] = []
    feature_rows: list[list[float]] = []
    labels: list[float] = []
    seen_addresses: set[str] = set()

    with path.open("r", newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames is None:
            raise ValueError("training CSV has no header row")

        missing = [feature for feature in FEATURE_NAMES if feature not in reader.fieldnames]
        if missing:
            raise ValueError(f"training CSV is missing feature columns: {missing}")
        if "label" not in reader.fieldnames:
            raise ValueError("training CSV is missing required label column")

        for row_number, row in enumerate(reader, start=2):
            try:
                address = (row.get("address") or "").strip()
                if not address:
                    raise ValueError("address is required")
                normalized_address = address.lower()
                if normalized_address in seen_addresses:
                    raise ValueError(
                        "duplicate address; one wallet must produce one training sample"
                    )

                feature_values = []
                for feature in FEATURE_NAMES:
                    raw_value = (row.get(feature) or "").strip()
                    if not raw_value:
                        raise ValueError(f"feature {feature!r} is empty")
                    value = float(raw_value)
                    if not math.isfinite(value):
                        raise ValueError(f"feature {feature!r} is not finite")
                    feature_values.append(value)

                seen_addresses.add(normalized_address)
                feature_rows.append(feature_values)
                labels.append(parse_label(row["label"]))
                addresses.append(address)
            except Exception as exc:
                raise ValueError(f"invalid training row {row_number}: {exc}") from exc

    if len(labels) < 6:
        raise ValueError("at least six unique labeled wallets are required")
    if len(set(labels)) != 2:
        raise ValueError("training data must include both label 1 and label 0")
    label_counts = {label: labels.count(label) for label in set(labels)}
    if any(count < 3 for count in label_counts.values()):
        raise ValueError(
            "each class needs at least three wallets for train/validation/test splits"
        )

    return Dataset(
        addresses=addresses,
        features=torch.tensor(feature_rows, dtype=torch.float32),
        labels=torch.tensor(labels, dtype=torch.float32),
    )


def split_dataset(
    dataset: Dataset,
    validation_ratio: float,
    test_ratio: float,
    seed: int,
) -> tuple[Dataset, Dataset, Dataset]:
    if not 0.0 < validation_ratio < 0.5:
        raise ValueError("validation ratio must be greater than 0 and less than 0.5")
    if not 0.0 < test_ratio < 0.5:
        raise ValueError("test ratio must be greater than 0 and less than 0.5")
    if validation_ratio + test_ratio >= 0.7:
        raise ValueError("validation and test ratios leave too little training data")

    groups: dict[int, list[int]] = {0: [], 1: []}
    for index, label in enumerate(dataset.labels.tolist()):
        groups[int(label)].append(index)

    train_indices: list[int] = []
    validation_indices: list[int] = []
    test_indices: list[int] = []
    rng = random.Random(seed)

    for label, indices in groups.items():
        rng.shuffle(indices)
        if len(indices) < 3:
            raise ValueError(f"class {label} needs at least three wallets")

        validation_count = max(1, int(round(len(indices) * validation_ratio)))
        test_count = max(1, int(round(len(indices) * test_ratio)))
        while len(indices) - validation_count - test_count < 1:
            if validation_count >= test_count and validation_count > 1:
                validation_count -= 1
            elif test_count > 1:
                test_count -= 1
            else:
                raise ValueError(f"class {label} has too few wallets for three splits")

        validation_indices.extend(indices[:validation_count])
        test_indices.extend(indices[validation_count : validation_count + test_count])
        train_indices.extend(indices[validation_count + test_count :])

    rng.shuffle(train_indices)
    rng.shuffle(validation_indices)
    rng.shuffle(test_indices)
    return (
        take_rows(dataset, train_indices),
        take_rows(dataset, validation_indices),
        take_rows(dataset, test_indices),
    )


def take_rows(dataset: Dataset, indices: list[int]) -> Dataset:
    tensor_indices = torch.tensor(indices, dtype=torch.long)
    return Dataset(
        addresses=[dataset.addresses[index] for index in indices],
        features=dataset.features.index_select(0, tensor_indices),
        labels=dataset.labels.index_select(0, tensor_indices),
    )


def fit_standardizer(features: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor]:
    means = features.mean(dim=0)
    stds = features.std(dim=0, unbiased=False)
    stds = torch.where(stds < 1e-6, torch.ones_like(stds), stds)
    return means, stds


def standardize(features: torch.Tensor, means: torch.Tensor, stds: torch.Tensor) -> torch.Tensor:
    return (features - means) / stds


def train_model(
    train: Dataset,
    validation: Dataset,
    test: Dataset,
    hidden_widths: list[int],
    args: argparse.Namespace,
) -> tuple[
    WalletRiskMlp,
    dict[str, float],
    torch.Tensor,
    torch.Tensor,
    dict[str, float | str],
]:
    torch.manual_seed(args.seed)
    means, stds = fit_standardizer(train.features)
    train_x = standardize(train.features, means, stds)
    validation_x = standardize(validation.features, means, stds)
    test_x = standardize(test.features, means, stds)

    model = WalletRiskMlp(train_x.shape[1], hidden_widths)
    positives = train.labels.sum().item()
    negatives = float(len(train.labels)) - positives
    pos_weight = torch.tensor([max(negatives / max(positives, 1.0), 1.0)])
    loss_fn = nn.BCEWithLogitsLoss(pos_weight=pos_weight)
    optimizer = torch.optim.AdamW(
        model.parameters(),
        lr=args.learning_rate,
        weight_decay=args.weight_decay,
    )

    loader = DataLoader(
        TensorDataset(train_x, train.labels),
        batch_size=args.batch_size,
        shuffle=True,
    )

    for _ in range(args.epochs):
        model.train()
        for batch_x, batch_y in loader:
            optimizer.zero_grad(set_to_none=True)
            logits = model(batch_x)
            loss = loss_fn(logits, batch_y)
            loss.backward()
            optimizer.step()

    model.eval()
    with torch.no_grad():
        train_logits = model(train_x)
        validation_logits = model(validation_x)
        test_logits = model(test_x)

    calibration = fit_platt_calibration(validation.labels, validation_logits)
    train_prob = calibrated_probabilities(train_logits, calibration)
    validation_prob = calibrated_probabilities(validation_logits, calibration)
    test_prob = calibrated_probabilities(test_logits, calibration)

    metrics = {
        **prefix_metrics("train", classification_metrics(train.labels, train_prob)),
        **prefix_metrics("validation", classification_metrics(validation.labels, validation_prob)),
        **prefix_metrics("test", classification_metrics(test.labels, test_prob)),
    }
    return model, metrics, means, stds, calibration


def fit_platt_calibration(
    labels: torch.Tensor,
    logits: torch.Tensor,
) -> dict[str, float | str]:
    raw_slope = torch.tensor(0.5413249, dtype=torch.float32, requires_grad=True)
    intercept = torch.tensor(0.0, dtype=torch.float32, requires_grad=True)
    optimizer = torch.optim.Adam([raw_slope, intercept], lr=0.03)
    loss_fn = nn.BCEWithLogitsLoss()

    detached_logits = logits.detach()
    for _ in range(400):
        optimizer.zero_grad(set_to_none=True)
        slope = torch.nn.functional.softplus(raw_slope)
        calibrated_logits = detached_logits * slope + intercept
        loss = loss_fn(calibrated_logits, labels)
        loss.backward()
        optimizer.step()

    return {
        "method": "platt",
        "slope": float(torch.nn.functional.softplus(raw_slope).detach().item()),
        "intercept": float(intercept.detach().item()),
    }


def calibrated_probabilities(
    logits: torch.Tensor,
    calibration: dict[str, float | str],
) -> torch.Tensor:
    slope = float(calibration["slope"])
    intercept = float(calibration["intercept"])
    return torch.sigmoid(logits * slope + intercept)


def classification_metrics(labels: torch.Tensor, probabilities: torch.Tensor) -> dict[str, float]:
    y_true = [float(item) for item in labels.tolist()]
    y_prob = [float(item) for item in probabilities.tolist()]
    y_pred = [1.0 if item >= 0.5 else 0.0 for item in y_prob]

    tp = sum(1 for truth, pred in zip(y_true, y_pred) if truth == 1.0 and pred == 1.0)
    tn = sum(1 for truth, pred in zip(y_true, y_pred) if truth == 0.0 and pred == 0.0)
    fp = sum(1 for truth, pred in zip(y_true, y_pred) if truth == 0.0 and pred == 1.0)
    fn = sum(1 for truth, pred in zip(y_true, y_pred) if truth == 1.0 and pred == 0.0)

    precision = safe_div(tp, tp + fp)
    recall = safe_div(tp, tp + fn)
    f1 = safe_div(2.0 * precision * recall, precision + recall)
    accuracy = safe_div(tp + tn, len(y_true))
    brier = sum((prob - truth) ** 2 for truth, prob in zip(y_true, y_prob)) / len(y_true)

    return {
        "accuracy": accuracy,
        "precision": precision,
        "recall": recall,
        "f1": f1,
        "auc": roc_auc(y_true, y_prob),
        "brier": brier,
    }


def safe_div(numerator: float, denominator: float) -> float:
    return 0.0 if denominator == 0 else numerator / denominator


def roc_auc(labels: list[float], probabilities: list[float]) -> float:
    positives = sum(1 for label in labels if label == 1.0)
    negatives = len(labels) - positives
    if positives == 0 or negatives == 0:
        return 0.5

    pairs = sorted(zip(probabilities, labels), key=lambda item: item[0])
    rank_sum = 0.0
    index = 0
    while index < len(pairs):
        end = index + 1
        while end < len(pairs) and pairs[end][0] == pairs[index][0]:
            end += 1
        average_rank = ((index + 1) + end) / 2.0
        rank_sum += average_rank * sum(
            1 for _, label in pairs[index:end] if label == 1.0
        )
        index = end

    return (rank_sum - positives * (positives + 1) / 2.0) / (positives * negatives)


def prefix_metrics(prefix: str, metrics: dict[str, float]) -> dict[str, float]:
    return {f"{prefix}_{key}": value for key, value in metrics.items()}


def export_artifact(
    model: WalletRiskMlp,
    means: torch.Tensor,
    stds: torch.Tensor,
    calibration: dict[str, float | str],
    args: argparse.Namespace,
) -> dict:
    hidden_layers = []
    for layer in model.hidden_layers:
        hidden_layers.append(
            {
                "activation": "relu",
                "weights": tensor_to_list(layer.weight.detach()),
                "bias": tensor_to_list(layer.bias.detach()),
            }
        )

    return {
        "model_type": "pytorch_mlp",
        "feature_schema_version": FEATURE_SCHEMA_VERSION,
        "feature_names": FEATURE_NAMES,
        "feature_means": tensor_to_list(means),
        "feature_stds": tensor_to_list(stds),
        "hidden_layers": hidden_layers,
        "output_weights": tensor_to_list(model.output_layer.weight.detach().squeeze(0)),
        "output_bias": float(model.output_layer.bias.detach().squeeze(0).item()),
        "calibration": calibration,
        "explanation_top_k": 12,
        "training": {
            "framework": "pytorch",
            "model_id": args.model_id,
            "model_version": args.model_version,
            "feature_schema_version": FEATURE_SCHEMA_VERSION,
        },
    }


def tensor_to_list(tensor: torch.Tensor) -> list:
    return json.loads(json.dumps(tensor.cpu().tolist()))


def write_outputs(
    output_dir: Path,
    artifact: dict,
    metrics: dict[str, float],
    train: Dataset,
    validation: Dataset,
    test: Dataset,
    dataset_sha256: str,
    args: argparse.Namespace,
) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    now_ms = int(time.time() * 1000)
    training_run_id = f"tron_wallet_train_{uuid.uuid4().hex}"
    deployment_id = f"tron_wallet_deploy_{uuid.uuid4().hex}"
    status = "ACTIVE" if args.activate else "CANDIDATE"
    model_quality_score = float(metrics.get("test_auc", 0.5))
    if args.activate:
        validate_activation_gate(metrics, test, args)

    parameters = {
        "epochs": args.epochs,
        "batch_size": args.batch_size,
        "learning_rate": args.learning_rate,
        "weight_decay": args.weight_decay,
        "hidden_widths": parse_hidden_widths(args.hidden_widths),
        "validation_ratio": args.validation_ratio,
        "test_ratio": args.test_ratio,
        "minimum_test_samples": args.minimum_test_samples,
        "minimum_test_auc": args.minimum_test_auc,
        "maximum_test_brier": args.maximum_test_brier,
        "seed": args.seed,
        "dataset_sha256": dataset_sha256,
    }

    artifact_path = output_dir / "model_artifact.json"
    metrics_path = output_dir / "metrics.json"
    feature_schema_path = output_dir / "feature_schema.json"
    register_sql_path = output_dir / "register_model.sql"

    artifact_json = json.dumps(artifact, indent=2, sort_keys=True)
    artifact_sha256 = hashlib.sha256(artifact_json.encode("utf-8")).hexdigest()
    metrics_json = json.dumps(metrics, indent=2, sort_keys=True)
    parameters_json = json.dumps(parameters, indent=2, sort_keys=True)

    artifact_path.write_text(artifact_json + "\n", encoding="utf-8")
    metrics_path.write_text(metrics_json + "\n", encoding="utf-8")
    feature_schema_path.write_text(
        json.dumps(
            {
                "feature_schema_version": FEATURE_SCHEMA_VERSION,
                "feature_names": FEATURE_NAMES,
                "label_column": "label",
                "label_policy": args.label_policy,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    register_sql_path.write_text(
        build_register_sql(
            artifact_json=artifact_json,
            metrics_json=metrics_json,
            parameters_json=parameters_json,
            artifact_uri=str(artifact_path),
            training_run_id=training_run_id,
            deployment_id=deployment_id,
            status=status,
            train=train,
            validation=validation,
            test=test,
            dataset_sha256=dataset_sha256,
            artifact_sha256=artifact_sha256,
            model_quality_score=model_quality_score,
            now_ms=now_ms,
            args=args,
        )
        + "\n",
        encoding="utf-8",
    )

    print(f"wrote {artifact_path}")
    print(f"wrote {metrics_path}")
    print(f"wrote {feature_schema_path}")
    print(f"wrote {register_sql_path}")
    print(json.dumps(metrics, indent=2, sort_keys=True))


def validate_activation_gate(
    metrics: dict[str, float],
    test: Dataset,
    args: argparse.Namespace,
) -> None:
    failures = []
    if len(test.labels) < args.minimum_test_samples:
        failures.append(
            f"test samples {len(test.labels)} < {args.minimum_test_samples}"
        )
    if metrics.get("test_auc", 0.0) < args.minimum_test_auc:
        failures.append(
            f"test AUC {metrics.get('test_auc', 0.0):.4f} < {args.minimum_test_auc:.4f}"
        )
    if metrics.get("test_brier", 1.0) > args.maximum_test_brier:
        failures.append(
            "test Brier "
            f"{metrics.get('test_brier', 1.0):.4f} > {args.maximum_test_brier:.4f}"
        )
    if failures:
        raise ValueError(
            "model activation gate failed: "
            + "; ".join(failures)
            + ". Export as CANDIDATE, improve the data/model, or explicitly adjust the gate."
        )


def build_register_sql(
    artifact_json: str,
    metrics_json: str,
    parameters_json: str,
    artifact_uri: str,
    training_run_id: str,
    deployment_id: str,
    status: str,
    train: Dataset,
    validation: Dataset,
    test: Dataset,
    dataset_sha256: str,
    artifact_sha256: str,
    model_quality_score: float,
    now_ms: int,
    args: argparse.Namespace,
) -> str:
    positive_count = int(
        train.labels.sum().item()
        + validation.labels.sum().item()
        + test.labels.sum().item()
    )
    total_count = len(train.labels) + len(validation.labels) + len(test.labels)
    negative_count = total_count - positive_count

    return f"""
INSERT INTO tron_db.wallet_ml_training_runs
(
    training_run_id,
    model_id,
    model_version,
    feature_schema_version,
    training_dataset_id,
    dataset_sha256,
    label_policy,
    train_sample_count,
    validation_sample_count,
    test_sample_count,
    positive_label_count,
    negative_label_count,
    metrics_json,
    parameters_json,
    artifact_uri,
    artifact_json,
    status,
    started_at_unix_ms,
    completed_at_unix_ms
)
VALUES
(
    {sql_string(training_run_id)},
    {sql_string(args.model_id)},
    {sql_string(args.model_version)},
    {sql_string(FEATURE_SCHEMA_VERSION)},
    {sql_string(args.dataset_id)},
    {sql_string(dataset_sha256)},
    {sql_string(args.label_policy)},
    {len(train.labels)},
    {len(validation.labels)},
    {len(test.labels)},
    {positive_count},
    {negative_count},
    {sql_string(metrics_json)},
    {sql_string(parameters_json)},
    {sql_string(artifact_uri)},
    {sql_string(artifact_json)},
    {sql_string(status)},
    {now_ms},
    {now_ms}
);

INSERT INTO tron_db.wallet_ml_model_registry
(
    model_id,
    model_version,
    model_family,
    feature_schema_version,
    calibration_version,
    artifact_json,
    artifact_sha256,
    metrics_json,
    training_run_id,
    training_dataset_id,
    label_policy,
    model_quality_score,
    status,
    trained_at_unix_ms,
    activated_at_unix_ms
)
VALUES
(
    {sql_string(args.model_id)},
    {sql_string(args.model_version)},
    'pytorch_mlp',
    {sql_string(FEATURE_SCHEMA_VERSION)},
    'platt_v1',
    {sql_string(artifact_json)},
    {sql_string(artifact_sha256)},
    {sql_string(metrics_json)},
    {sql_string(training_run_id)},
    {sql_string(args.dataset_id)},
    {sql_string(args.label_policy)},
    {model_quality_score},
    {sql_string(status)},
    {now_ms},
    {now_ms if status == "ACTIVE" else 0}
);
{build_deployment_sql(deployment_id, now_ms, args) if status == "ACTIVE" else ""}
""".strip()


def build_deployment_sql(
    deployment_id: str,
    now_ms: int,
    args: argparse.Namespace,
) -> str:
    return f"""

INSERT INTO tron_db.wallet_ml_model_deployments
(
    environment,
    feature_schema_version,
    deployment_id,
    model_id,
    model_version,
    status,
    deployed_by,
    notes,
    deployed_at_unix_ms
)
VALUES
(
    'production',
    {sql_string(FEATURE_SCHEMA_VERSION)},
    {sql_string(deployment_id)},
    {sql_string(args.model_id)},
    {sql_string(args.model_version)},
    'ACTIVE',
    'pytorch_training_pipeline',
    'Passed configured activation gates',
    {now_ms}
);
""".strip()


def sql_string(value: str) -> str:
    return "'" + value.replace("\\", "\\\\").replace("'", "''") + "'"


def main() -> None:
    args = parse_args()
    random.seed(args.seed)
    torch.manual_seed(args.seed)

    input_path = Path(args.input)
    dataset_sha256 = hashlib.sha256(input_path.read_bytes()).hexdigest()
    dataset = read_training_csv(input_path)
    train, validation, test = split_dataset(
        dataset,
        args.validation_ratio,
        args.test_ratio,
        args.seed,
    )
    model, metrics, means, stds, calibration = train_model(
        train,
        validation,
        test,
        parse_hidden_widths(args.hidden_widths),
        args,
    )
    artifact = export_artifact(model, means, stds, calibration, args)
    write_outputs(
        Path(args.output_dir),
        artifact,
        metrics,
        train,
        validation,
        test,
        dataset_sha256,
        args,
    )


if __name__ == "__main__":
    main()
