# TRON Wallet Risk PyTorch Model

This folder trains the first simple proprietary AML model for TRON wallet risk.
The runtime contract is:

1. Rust builds wallet feature snapshots from ClickHouse evidence.
2. Python/PyTorch trains a small tabular MLP from labeled examples.
3. Python exports `model_artifact.json`.
4. The artifact is inserted into `tron_db.wallet_ml_model_registry`.
5. Rust loads the ACTIVE artifact and returns a laundering probability.

## Quick Smoke Test

The sample CSV is only for checking that the pipeline works. Do not treat it as
a real AML model.

```powershell
cd D:\Sarbazi\dockerizd_eth_code
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -r ml\tron_wallet_risk\requirements.txt
python ml\tron_wallet_risk\train.py --input ml\tron_wallet_risk\sample_training_data.csv --output-dir ml\tron_wallet_risk\artifacts\smoke
```

The script writes:

```text
model_artifact.json
metrics.json
feature_schema.json
register_model.sql
```

The smoke dataset is too small to pass the production activation gate. Its
generated SQL registers a `CANDIDATE`.

## Export From ClickHouse

After `wallet_ml_labels` and `wallet_ml_feature_snapshots` have data, export a
training CSV with:

```powershell
clickhouse-client --query "$(Get-Content ml\tron_wallet_risk\export_training_dataset.sql -Raw) FORMAT CSVWithNames" > ml\tron_wallet_risk\training.csv
```

Then train:

```powershell
python ml\tron_wallet_risk\train.py --input ml\tron_wallet_risk\training.csv --output-dir ml\tron_wallet_risk\artifacts\tron_wallet_pytorch_mlp_v1
```

## Build Training CSV From Labeled Addresses

If you already have labeled wallets, start with this CSV:

```text
address,label
TWalletAddress1,1
TWalletAddress2,0
```

`1` means suspicious or laundering-related. `0` means clean or benign.

Start the Rust API first:

```powershell
cd D:\Sarbazi\dockerizd_eth_code\app
cargo run --bin tron_graph_api
```

In another terminal, generate model features for each labeled wallet:

```powershell
cd D:\Sarbazi\dockerizd_eth_code
python ml\tron_wallet_risk\build_training_csv_from_api.py --labels ml\tron_wallet_risk\my_labeled_wallets.csv --output ml\tron_wallet_risk\training.csv --labels-sql-output ml\tron_wallet_risk\insert_labels.sql
```

This calls:

```text
/api/tron/wallet/{address}/ai-risk
```

The API will build the wallet fingerprint, exposure features, and persist a
feature snapshot. Even if there is no ACTIVE model yet, it still returns the
features needed for training.

Then train and review a candidate:

```powershell
python ml\tron_wallet_risk\train.py --input ml\tron_wallet_risk\training.csv --output-dir ml\tron_wallet_risk\artifacts\tron_wallet_pytorch_mlp_v1
```

For a real dataset, activate only after reviewing the held-out test metrics:

```powershell
python ml\tron_wallet_risk\train.py --input ml\tron_wallet_risk\training.csv --output-dir ml\tron_wallet_risk\artifacts\tron_wallet_pytorch_mlp_v1 --activate
```

Activation defaults require at least 200 held-out test wallets, test AUC of at
least 0.70, and test Brier score no greater than 0.25. The generated SQL stores
the checksummed artifact and updates the production deployment pointer.

Run the generated SQL against ClickHouse:

```text
ml\tron_wallet_risk\artifacts\tron_wallet_pytorch_mlp_v1\register_model.sql
```

## Training Data Format

Training data is a CSV with exactly one row per unique labeled wallet snapshot.
Duplicate addresses, missing values, and non-finite values are rejected.
Generated CSVs also retain the feature snapshot id, schema version, generation
time, and available label provenance so a training run can be reproduced.

Required columns:

```text
address
label
total_transfers_log
unique_transactions_log
incoming_transfers_log
outgoing_transfers_log
unique_senders_log
unique_receivers_log
fan_in_score
fan_out_score
flow_imbalance_score
burst_score
swap_ratio
bridge_ratio
exchange_interaction_ratio
contract_call_ratio
counterparty_concentration
token_diversity_score
exposure_score
exposure_source_count_score
exposure_path_count_score
exposure_min_hop_score
identity_confidence
exchange_service_wallet_score
truncated_sample_score
data_volume_score
```

Label meaning:

```text
1 = laundering / illicit / suspicious confirmed by evidence
0 = benign / clean / normal confirmed by evidence
```

Use confirmed labels. Do not train on the old rule score as the target, because
that only teaches the model to imitate the old formula.

## Recommended Label Sources

Good positive examples:

```text
confirmed laundering wallets
scam cashout wallets
sanctioned or enforcement-listed wallets
wallets linked to confirmed illicit clusters
bridge/swap layering cases confirmed by analysts
```

Good negative examples:

```text
normal retail wallets
known exchange hot wallets labeled as service wallets
known exchange deposit wallets not involved in a bad case
normal DEX users
merchant or operational wallets with benign history
```

## Important Training Rule

The feature snapshot should be generated from data available before or at the
label decision time. Do not include future transactions that happened after the
wallet was labeled, or the model will learn from leaked future information.

The trainer uses label-stratified train/validation/test partitions. The model is
fit on training data, Platt calibration is fit on validation logits, and final
metrics are measured on the untouched test set. For production evaluation,
partition related wallets by entity/case/cluster and use time-based holdouts as
well; address-only random splitting can leak related behavior across partitions.
