# Continuous Bitcoin analytics

AML-08 runs the existing Bitcoin investigation pipeline continuously over
**bounded, repeated sweeps of canonical ingested addresses**. It does not read
Bitcoin RPC, import intelligence, approve a claim or review a clustering lead.
Each network still owns its independent ClickHouse/database on its own VM.
Shared schema v5 and the risk policy/weights are unchanged.

## Work and persisted outputs

One sequential job refreshes one address using the same readers, source fences,
detectors and publication checks as an interactive investigation:

| Work | Existing destination |
|---|---|
| Activity fingerprint and semantic evidence | `inputs.fingerprint` and `inputs.semantic_events` in the existing `aml_wallet_risk_assessments.evidence_json` |
| Common-input/change leads | Existing cluster run, claim, membership and version tables; complete bounded output is sealed |
| Reviewed-seed exposure | Existing exposure run, path, hop, seed and summary tables; existing completeness/dependency checks apply |
| Evidence risk assessment | Existing policy/rule and assessment/signal tables with publication seal |

The worker does not invent a per-transaction feature row from an address
aggregate. `aml_transaction_features` is not populated by this worker. Native
single-transaction semantic facts continue to be written with their block;
historical fact backfill/reprocessing remains ING-04. Full daily activity arrays
and graph display data are still computed by the interactive API, not stored as
a new worker-owned warehouse family.

Operational receipts hold hashes, output IDs, source snapshot and coverage.
They are references for inspection, not current risk decisions. Always use the
existing risk/exposure/cluster inspection endpoints to revalidate dependencies.
Partial coverage remains partial; a completed job does not mean a complete
search, a fully analyzed chain, or a laundering finding.

## Scheduling and recovery

- Canonical input/output addresses are discovered in lexicographic keyset pages.
  Script-only outputs never become address jobs. The sweep has a fixed discovery
  upper height, while each analysis uses the then-current canonical snapshot.
- The queue has at most **128** jobs, discovery pages at most **100**, and recent
  successful receipts at most **16**. Existing per-reader query limits remain
  active. Discovery caps ClickHouse to two threads, 256 MiB and 15 seconds.
- One job runs at a time, with 25 recent activity roots and a configurable
  deadline (default 120 seconds). A timeout may leave independently published
  sub-results or unsealed audit rows. Retrying creates new analysis run IDs;
  execution is **at least once**, not exactly once.
  The Compose worker defaults to a 512 MiB container memory limit, one CPU and
  at most 64 processes. Container limits are configurable separately from query
  budgets; they do not establish production throughput.
- The cursor advances only together with the enqueued jobs in a durable
  checkpoint. A restarted process keeps that cursor/queue. An interrupted job
  becomes retryable or failed when its attempt budget is exhausted.
- Retry uses bounded exponential backoff. Due new jobs take priority over
  retries, so one failed address does not block unrelated jobs. Exhausted jobs
  stay failed until explicit operator retry. They consume queue space; if all
  128 slots are occupied, discovery stops with an explicit backpressure reason.
- Failed jobs do not prevent the next scheduled sweep of other addresses.
  Failed addresses are skipped until retried. Recent receipt history is bounded;
  immutable analytic runs remain in ClickHouse under their existing policies.
- Reorg of the discovery anchor, rollback below it or a changed history start
  abandons the pending queue and restarts discovery on the current branch.
  Changed detector/configuration definitions also restart the sweep. The count
  of abandoned jobs is retained; this operational checkpoint is not an immutable
  forensic job-event journal.
- After a sweep drains, the refresh interval starts (default 300 seconds).
  A new sweep sees newly ingested addresses and changed/revoked intelligence.
  There is no event-driven intelligence notification in this stage. End-to-end
  refresh latency includes the sweep duration; it is not a five-minute SLA.
- New blocks do not restart a healthy in-progress discovery pass, avoiding
  starvation of later addresses. New addresses sorting before the current cursor
  or above the fixed discovery height are covered on a later sweep.

The initial version is `bitcoin_analytics_worker_v1`. Heartbeats are saved every
five seconds while a job runs and each polling cycle while waiting. A fresh
heartbeat proves process progress, not fresh or complete blockchain analysis.
Waiting for the first committed block is healthy liveness with unavailable data.

## Operational checkpoint

`BITCOIN_ANALYTICS_STATE_DIR` is an absolute persistent local directory. Compose
mounts `aml_bitcoin_analytics_state` at `/var/lib/bitcoin-analytics` for the worker
and read-only for the API. The file is scoped to the network/database, bounded
to 1 MiB and checksum-checked. Updates write/fsync a temporary file, rename it
atomically, then sync the directory on Linux. A corrupt checkpoint fails closed;
it is not silently reset. Preserve it for diagnosis and restore a known good
checkpoint, or intentionally start a new sweep after stopping the worker.

The independent kernel lock uses the existing shared
`BITCOIN_WRITER_LOCK_DIR` with a database-specific analytics lock. It does not
hold the ingestion or review lock. Every analytics process on this VM must
share both the same lock filesystem and checkpoint volume. It is not a
distributed lease or a multi-host scheduler. Copying the checkpoint to another
database/server without verifying its history is not a supported migration.

The operational checkpoint is **not** a second blockchain database. Analytic
evidence stays in ClickHouse, and central investigations stay in Neo4j. Losing
the checkpoint requires rediscovery and can duplicate attempts, not deletion of
published chain facts. Back up it with the worker stopped and retain the
ClickHouse backup separately.

## Run and inspect

Existing installations need a newly built/imported application image for this
stage, including the state-directory ownership set in the Dockerfile. Reusing
an older image will not add the worker commands. No new schema migration is
required. Build the image through the existing source-build workflow or import
the updated offline image before the startup examples below.

```bash
# Full Bitcoin VM startup now includes ingestion and analytics.
bash scripts/vm.sh bitcoin up --pull-never
# Explicit API-only startup does not launch background workers.
bash scripts/vm.sh bitcoin up --api-only --pull-never

docker compose -f dockerizd_bitcoin/docker-compose.yml exec -T bitcoin-api \
  bitcoin_aml analytics status
# Liveness only; does not prove complete/current data.
docker compose -f dockerizd_bitcoin/docker-compose.yml exec -T bitcoin-analytics \
  bitcoin_aml analytics healthcheck

# Stop both ingestion and analytics; retain API, databases and state volumes.
bash scripts/vm.sh bitcoin pause-ingestion
bash scripts/vm.sh bitcoin resume
```

Use `--env-file dockerizd_bitcoin/.env` and the deployment's existing project name
with direct Compose commands when required by the environment. Do not create a
second production stack to run these examples. The local runner's
`--with-ingestion` mode and VM launcher include `bitcoin-analytics`; API-only
startup retains its existing behavior.

For native execution:

```bash
bitcoin_aml analytics --help
bitcoin_aml analytics
bitcoin_aml analytics once --max-jobs 5
bitcoin_aml analytics status
bitcoin_aml analytics healthcheck
```

`once` attempts up to the specified number of due jobs and stops when there is
no immediately runnable work. It does not wait through retry backoff. Exit 0
means its attempted jobs succeeded (or no jobs were due), 2 means an attempt
failed, and 1 is a configuration/storage/source failure. Continuous mode keeps
retrying source/discovery outages while preserving the cursor. SIGTERM/CTRL-C
checkpoints interrupted work before exit. An unclean kill relies on heartbeat
expiry and startup recovery.

After stopping the analytics process, an operator can retry an exhausted job:

```bash
docker compose --env-file dockerizd_bitcoin/.env \
  -f dockerizd_bitcoin/docker-compose.yml stop bitcoin-analytics
docker compose --env-file dockerizd_bitcoin/.env \
  -f dockerizd_bitcoin/docker-compose.yml --profile tools run --rm --no-deps \
  bitcoin-analytics-tools bitcoin_aml analytics retry JOB_ID_FROM_STATUS
docker compose --env-file dockerizd_bitcoin/.env \
  -f dockerizd_bitcoin/docker-compose.yml up -d --no-deps bitcoin-analytics
```

No arbitrary score, label or approval is accepted by these commands.

## API and display

`GET /api/bitcoin/analytics/status` requires `X-AML-Service-Key`. The general
protected `/status` and investigation `data_coverage.analytics_worker` include
the local worker status too. Missing/unreadable state is explicit and does not
disable interactive investigation. The panel labels this as **captured** worker
status, including for saved investigations; it is not a live health dashboard.
`source_matches_now` on the dedicated status endpoint is only a source comparison;
`requires_dependency_revalidation` remains true.

Configuration defaults are in `.env.example`. Increasing budgets does not prove
capacity. This bounded address-sweep implementation needs real-node/mainnet,
production load and five-VM acceptance. Alerts, watchlists, case management,
cross-chain tracing and historical replay are separate work.

See [validation](ANALYTICS_VALIDATION.md) and the [remaining tasks](../TODO_BITCOIN.md).
