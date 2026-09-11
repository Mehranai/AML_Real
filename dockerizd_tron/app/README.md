# TRON AML Service

> Current UI workflow: ClickHouse evidence -> main VM Analytical Node -> one central Neo4j.
> Export preserves the displayed snapshot. ML remains disabled; central evidence scoring is active.
> Follow [Central investigations](../../docs/CENTRAL_INVESTIGATIONS_FA.md);
> local projection and ML sections below are retained as legacy/reference material.

> New location: `AML_Whole/dockerizd_tron/app`. The shared network selector and
> startup commands are documented in [AML Whole](../../README.md).

Rust services for TRON canonical ingestion, wallet investigation, graph
projection, exposure analysis, and PyTorch-backed AML inference. The master
architecture and remaining production gaps are documented in:

```text
../docs/explain_tron.md
```

## Docker Deployment

`docker-compose.yml` is the canonical deployment for the complete local TRON
stack. It uses one multi-stage, non-root Rust image for all application
processes and separate containers for each long-running responsibility.

### First installation on any machine

Install Docker Engine and Compose v2 on Linux, copy the repository,
then run:

```bash
cd "$HOME/AML_Whole/dockerizd_tron/app"
cp -n .env.example .env
```

Edit `.env` and replace the ClickHouse and Neo4j passwords. Set `TRON_RPC_URL`
and, when the provider requires it, `TRON_API_KEY`. The real `.env` is ignored
by Git and excluded from the Docker build context.

Build and start all services:

```bash
docker compose up -d --build
docker compose ps
```

Compose waits for the ClickHouse schema and Neo4j to become healthy before it
starts the dependent Rust services. On an empty ClickHouse volume,
`sql/init_database_tron.sql` is applied automatically; no schema binary is
required.

Services and host ports:

| Service | Responsibility | Host endpoint |
| --- | --- | --- |
| `clickhouse` | canonical AML evidence | HTTP `127.0.0.1:18123`, native `127.0.0.1:19000` |
| `neo4j` | rebuildable graph projection | Browser `http://127.0.0.1:18474`, Bolt `127.0.0.1:17687` |
| `tron-api` | API and investigation UI | `http://127.0.0.1:4001` |
| `tron-ingestion` | finalized-block ingestion | no published port |
| `tron-token-metadata-worker` | TRC20 metadata enrichment | no published port |

The ClickHouse server still listens on its standard native port `9000` inside
the Compose network. Docker publishes it as host port `19000`; nothing binds
host port `9000`.

Readiness and logs:

```bash
curl --fail --silent --show-error http://127.0.0.1:4001/ready
docker compose logs -f tron-api
docker compose logs -f tron-ingestion
docker compose logs -f tron-token-metadata-worker
```

Rebuild only the Rust services after code changes:

```bash
docker compose build tron-api
docker compose up -d tron-api tron-ingestion tron-token-metadata-worker
```

Stop the stack without deleting data:

```bash
docker compose down
```

`docker compose down --volumes` permanently deletes both databases and is only
appropriate for disposable development data.

### Infrastructure-only development

To run Rust from RustRover or Cargo while keeping only the databases in Docker:

```bash
docker compose up -d clickhouse neo4j
cargo run
# Second terminal
cargo run --bin tron_graph_api
# Optional third terminal
cargo run --bin tron_token_metadata_worker
```

The local process reads host addresses from `.env`. Containers receive internal
DNS addresses (`clickhouse:8123` and `neo4j:7687`) directly from Compose.

### Operational CLI commands in the image

The image contains every release binary, so maintenance jobs do not require a
Rust toolchain on the destination machine. For example:

```bash
docker compose run --rm tron-ingestion tron_replay_blocks 84890000 84890099
docker compose run --rm tron-ingestion tron_propagate_exposure
docker compose run --rm tron-ingestion tron_discover_address_clusters 0 1000000 5000
```

For JSON/JSONL input jobs, mount the input file read-only with `-v` and pass its
container path to the selected binary.

### Moving from TronGrid to a local Full Node

Only `TRON_RPC_URL` changes. On Linux, use the node's reachable private IP.
If the node runs as another Compose service on the same network, use that
service's DNS name and internal port. No ClickHouse, classifier, or investigation code needs to change.

## Investigation APIs

```text
GET  /api/tron/wallet/{address}/graph
GET  /api/tron/wallet/{address}/holdings
GET  /api/tron/wallet/{address}/fingerprint
GET  /api/tron/wallet/{address}/ai-risk
GET  /api/tron/wallet/{address}/investigation
GET  /api/tron/wallet/{source}/paths/{target}
GET  /api/analysis/tron/wallet/{address}
POST /api/tron/wallet/{address}/neo4j/import
```

GET requests read ClickHouse and do not mutate Neo4j. Use the explicit POST
route to project a wallet subgraph.

Example:

```bash
address="TRON_WALLET_ADDRESS"
curl --fail --silent --show-error "http://127.0.0.1:4001/api/tron/wallet/$address/investigation?depth=3&limit=500"
```

## Protocol and Contract Classification

Ingestion classifies each transaction using ordered, explainable evidence:

1. official bootstrap addresses and approved `address_entity` labels;
2. native TRON contract types such as staking and resource delegation;
3. known method selectors;
4. conservative multi-asset flow evidence;
5. TRC-20, TRC-721, and TRC-1155 transfer evidence emitted by the target contract.

The result covers `DEX`, `BRIDGE`, `LENDING`, `STAKING`, `MIXER`, `TOKEN`,
`NFT`, `SCAM`, `WALLET`, and `UNKNOWN`. Address-registry evidence wins over
heuristics. Agreeing evidence raises confidence; conflicting evidence is
recorded in `transaction_features.classification_source` with a `conflict_`
prefix. `is_contract_call` remains the actual execution form and is never
inferred from an address label.

Do not hardcode unreviewed scam, mixer, bridge, or service addresses. Submit an
entity label through the governed intelligence workflow below and approve it.
The approved projection is read from `address_entity`; ingestion refreshes its
in-memory cache every `TRON_PROTOCOL_REGISTRY_REFRESH_BLOCKS` blocks (default
`100`). Set the value to `0` only when runtime refresh must be disabled.

## Entity Intelligence, Clustering, and Exposure

Register each governed intelligence source before importing its labels:

```bash
cargo run --bin tron_register_intelligence_source -- ./source.json
```

Submit replay-safe entity claims from JSONL. New claims remain pending unless
the record contains an explicit independent reviewer:

```bash
cargo run --bin tron_ingest_entity_labels -- ./labels.jsonl
```

Discover pending exchange-deposit and service cluster claims from stored
canonical transfers, then approve or reject them explicitly:

```bash
cargo run --bin tron_discover_address_clusters -- 0 1000000 5000
cargo run --bin tron_review_intelligence -- CLUSTER_CLAIM CLAIM_ID APPROVED REVIEWER "Review reason"
```

The unified investigation response and UI expose active attribution, cluster
versions, source trust, evidence, and pending reviews. Full operator formats
and governance behavior are documented in
`../docs/tron_entity_intelligence.md`.

Propagate exposure:

```bash
cargo run --bin tron_propagate_exposure
```

## PyTorch Risk Model (Disabled for Now)

Build one feature row per unique labeled wallet:

```bash
cd "$HOME/AML_Whole/dockerizd_tron"
python3 ml/tron_wallet_risk/build_training_csv_from_api.py \
  --labels ml/tron_wallet_risk/my_labeled_wallets.csv \
  --output ml/tron_wallet_risk/training.csv
```

Train a candidate:

```bash
python3 ml/tron_wallet_risk/train.py \
  --input ml/tron_wallet_risk/training.csv \
  --output-dir ml/tron_wallet_risk/artifacts/candidate_v1
```

Review the untouched test metrics. `--activate` generates a production
deployment only when the configured sample-count, AUC, and Brier gates pass:

```bash
python3 ml/tron_wallet_risk/train.py \
  --input ml/tron_wallet_risk/training.csv \
  --output-dir ml/tron_wallet_risk/artifacts/model_v1 \
  --model-version v1 \
  --activate
```

The training and model-registration tools are retained, but runtime inference is
intentionally disabled while graph and evidence workflows are tested. Wallet
APIs return no laundering probability and do not substitute a formula. Re-enable
inference as a deliberate implementation phase after the evidence pipeline is
accepted; there is no runtime environment toggle in the current build.

Detailed ML instructions:

```text
../ml/tron_wallet_risk/README.md
```

## Ingestion Recovery

Every fetched solid block is journaled in `ingested_blocks` as `PROCESSING`,
`FAILED`, or `COMPLETE`. Transaction and block failures are stored in
`ingestion_failures` with a stable identity, attempt count, retryability, and
resolution status.

Replay one finalized block or an inclusive range:

```bash
cargo run --bin tron_replay_blocks -- 84890000
cargo run --bin tron_replay_blocks -- 84890000 84890099
```

Replay is limited to 10,000 blocks per command, accepts only blocks at or below
the current solid head, and never changes `sync_state`. Existing evidence is
logically deduplicated by canonical event IDs. A finalized block hash conflict
is recorded and stops ingestion; the replay command does not rewrite competing
histories.

Inspect unresolved failures:

```sql
SELECT *
FROM tron_db.ingestion_failures FINAL
WHERE status = 'OPEN'
ORDER BY last_failed_at_unix_ms DESC;
```

The detailed TRON completion checklist is
[`docs/tron_completion_todo.md`](../docs/tron_completion_todo.md).

## Historical Benchmark and Monitoring

Run bounded historical ingestion without replaying blocks already marked
`COMPLETE`:

```bash
cargo run --bin tron_benchmark_ingestion -- 2036 2040
cargo run --bin tron_benchmark_ingestion -- 2036 2040 TRON_WALLET_ADDRESS
```

The command accepts at most 10,000 blocks. It persists one compact row in
`ingestion_benchmarks`, including source kind, completed blocks, transactions,
elapsed time, throughput, rows, compressed bytes, active parts, and optional
unified-investigation latency. Benchmark history expires after 365 days;
canonical blockchain evidence does not expire.

Inspect ingestion health:

```text
GET /api/tron/ingestion/health
GET /api/tron/ingestion/health?gap_window_blocks=1000&stale_after_seconds=600&max_lag_blocks=20
```

The response reports the solid head, checkpoint, lag, stale processing blocks,
failed blocks, open failures, concrete missing journal ranges, and core
ClickHouse rows/bytes/parts.

Batch tuning variables:

```text
TRON_INGESTION_BATCH_MAX_ROWS=10000
TRON_INGESTION_FLUSH_INTERVAL_SECONDS=120
```

Blocks are still flushed immediately on successful completion. The interval is
only a background safety flush and is deliberately longer to avoid tiny parts
while remote receipts are slow. See
[`docs/tron_performance_baseline.md`](../docs/tron_performance_baseline.md) for
the measured baseline and local-node follow-up.

## Schema Safety

The complete active ClickHouse contract is stored in one file:
`sql/init_database_tron.sql`. Docker mounts it into
`/docker-entrypoint-initdb.d` and applies it only when the ClickHouse data
directory is empty.

Rust ingestion, replay, benchmark, and API processes are validation-only. They
verify required tables, views, and columns before starting; they never create,
alter, or drop database objects.

To rebuild a local test database, remove only the `app_clickhouse_data` Docker
volume and start ClickHouse again. This permanently deletes local ClickHouse
data; the Neo4j volume is independent and must not be removed.

Once production data is retained, schema evolution must use an automated,
versioned deployment migration. Recreating the volume is only appropriate while
the database contains disposable test data.

## Production Configuration

Local defaults are intentionally convenient for the supplied Docker services.
Production must inject ClickHouse, Neo4j, and node credentials through a secret
manager.

Important variables:

```text
CLICKHOUSE_URL
CLICKHOUSE_USER
CLICKHOUSE_PASSWORD
TRON_RPC_URL
TRON_API_KEY
NEO4J_URI
NEO4J_USERNAME
NEO4J_PASSWORD
```

Never expose ClickHouse, Neo4j, or a node RPC endpoint publicly without
authentication, network policy, TLS, and monitoring.

`app/.env` is currently present in repository history. Before any public or
shared deployment, remove it from version control/history and rotate every
credential or API key it has contained. `.gitignore` now prevents new `.env`
files from being added accidentally.
