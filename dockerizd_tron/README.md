# TRON AML Platform

> Current deployment has no TRON-local Neo4j. The main VM stores temporary
> investigations, makes them permanent through Export, and scores evidence without ML.
> See [Central investigations](../docs/CENTRAL_INVESTIGATIONS_FA.md).

> This project now lives at `AML_Whole/dockerizd_tron`. For the shared TRON/Ethereum
> entry page and Docker launcher, use [AML Whole](../README.md).
> Network-local commands still run from this project's `app` directory.

TRON-only anti-money-laundering ingestion and wallet-investigation platform.
ClickHouse is the evidence source of truth, Neo4j is an explicit graph
projection, and the Rust services provide finalized-block ingestion, token
metadata enrichment, APIs, and the investigation UI.

## Run the Complete Stack

Prerequisites: Docker Engine on Linux with Docker Compose v2.

```bash
cd "$HOME/AML_Whole/dockerizd_tron/app"
cp -n .env.example .env   # first installation only
# Edit .env and set the passwords and TRON RPC/API credentials.
docker compose up -d --build
docker compose ps
```

Open the investigation UI at `http://127.0.0.1:4001/`.

The default stack contains:

```text
clickhouse                 HTTP 18123, native TCP 19000
neo4j                      Browser 18474, Bolt 17687
tron-api                   API and UI on 4001
tron-ingestion             finalized TRON block ingestion
tron-token-metadata-worker token metadata enrichment
```

ClickHouse's standard container port remains `9000` inside the private Compose
network. Only host port `19000` is published, so port `9000` is no longer bound
on the machine.

Check readiness and logs:

```bash
curl --fail --silent --show-error http://127.0.0.1:4001/ready
docker compose logs -f tron-api tron-ingestion tron-token-metadata-worker
```

Stop containers while retaining ClickHouse and Neo4j data:

```bash
docker compose down
```

Do not add `--volumes` unless the stored databases are intentionally being
deleted.

## Infrastructure Only

For local Rust development outside containers:

```bash
docker compose up -d clickhouse neo4j
cargo run
# In a second terminal:
cargo run --bin tron_graph_api
```

The local `.env` uses host ports, while Compose overrides service-to-service
addresses with `clickhouse:8123` and `neo4j:7687`.

## Repository Layout

```text
app/                   Rust services, UI, SQL, Dockerfile, and Compose stack
docs/                  TRON architecture and operating documentation
ml/tron_wallet_risk/   TRON wallet-risk training pipeline (runtime disabled)
nodee/Tron/            local TRON node configuration for the future node source
```

Detailed commands and operating notes are in [`app/README.md`](app/README.md).


## Monitor TRON Ingestion and Inspect ClickHouse

Run these commands from the TRON application directory:

```bash
cd "$HOME/AML_Whole/dockerizd_tron/app"
docker compose up -d
docker compose ps
```

`docker compose ps` should show `clickhouse`, `neo4j`, `tron-api`,
`tron-ingestion`, and `tron-token-metadata-worker` as running or healthy.
Use `docker compose up -d --build` when application code or the Docker image has
changed.

### Watch Ingestion

Follow finalized-block ingestion in real time:

```bash
docker compose logs -f --tail 100 tron-ingestion
```

Press `Ctrl+C` to stop following the output. This does not stop the container.
The other service logs can be inspected separately:

```bash
docker compose logs -f --tail 100 tron-api
docker compose logs -f --tail 100 tron-token-metadata-worker
```

The API exposes both service readiness and detailed ingestion health:

```bash
curl --fail --silent --show-error http://127.0.0.1:4001/ready | jq .

curl --fail --silent --show-error http://127.0.0.1:4001/api/tron/ingestion/health | jq .
```

Important ingestion-health fields are:

- `latest_solid_block`: latest finalized block reported by the TRON source.
- `checkpoint_block`: last block committed successfully by ingestion.
- `lag_blocks`: distance between the source and the local checkpoint.
- `missing_block_count`: missing completed blocks in the inspected journal window.
- `open_failures`: unresolved records in the ingestion failure journal.
- `status`: overall status calculated from lag, gaps, and failures.

Open the investigation UI at `http://127.0.0.1:4001/`.

### Open the ClickHouse Console

This command reads the credentials from the container environment and opens
`tron_db`:

```bash
docker compose exec clickhouse sh -lc 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --database tron_db'
```

List all tables:

```sql
SHOW TABLES;
```

Inspect the current ingestion checkpoint:

```sql
SELECT
    chain,
    argMax(last_synced_block, updated_at) AS last_synced_block,
    max(updated_at) AS updated_at
FROM sync_state
GROUP BY chain
FORMAT Vertical;
```

Inspect the most recently recorded blocks:

```sql
SELECT
    block_number,
    block_hash,
    transaction_count,
    finality_status,
    ingestion_status,
    error_message,
    indexed_at_unix_ms
FROM ingested_blocks FINAL
WHERE chain = 'tron'
ORDER BY block_number DESC
LIMIT 20;
```

Count stored transactions and inspect recent rows:

```sql
SELECT
    count() AS transaction_count,
    max(block_number) AS latest_transaction_block
FROM transactions_canonical;

DESCRIBE TABLE transactions_canonical;

SELECT *
FROM transactions_canonical
ORDER BY block_number DESC
LIMIT 5
FORMAT Vertical;
```

Check the canonical fund-flow edges used by graph and investigation queries:

```sql
SELECT
    count() AS relationship_count,
    max(block_number) AS latest_relationship_block
FROM address_relationships_canonical;

SELECT *
FROM address_relationships_canonical
ORDER BY block_number DESC
LIMIT 5
FORMAT Vertical;
```

Inspect unresolved ingestion errors:

```sql
SELECT
    block_number,
    tx_hash,
    stage,
    error_class,
    error_message,
    retryable,
    attempt_count,
    status,
    last_failed_at_unix_ms
FROM ingestion_failures FINAL
WHERE chain = 'tron' AND status = 'OPEN'
ORDER BY last_failed_at_unix_ms DESC
LIMIT 20;
```

Show physical row counts and disk usage:

```sql
SELECT
    table,
    sum(rows) AS rows,
    formatReadableSize(sum(bytes_on_disk)) AS disk_size
FROM system.parts
WHERE active AND database = currentDatabase()
GROUP BY table
ORDER BY sum(bytes_on_disk) DESC;
```

Use `exit` or `Ctrl+D` to leave `clickhouse-client`.

### Connect with DBeaver

```text
Host:      127.0.0.1
HTTP port: 18123
Native TCP port: 19000
Database:  tron_db
Username:  value of CLICKHOUSE_USER in app/.env
Password:  value of CLICKHOUSE_PASSWORD in app/.env
```

Host port `19000` maps to ClickHouse port `9000` inside Docker. Containers in
the Compose network continue to connect to `clickhouse:9000`.

The main operational tables are:

- `sync_state`: durable checkpoint used by automatic resume.
- `ingested_blocks`: per-block processing and completion journal.
- `ingestion_failures`: retryable and non-retryable ingestion errors.
- `transactions`: stored TRON transactions and execution evidence.
- `address_relationships`: canonical fund-flow edges used to build graphs.
- `transaction_features`: normalized behavioral and semantic features.
- `semantic_aml_events`: AML-relevant events such as swaps and bridges.
- `token_metadata`: token names, symbols, decimals, and verification state.

