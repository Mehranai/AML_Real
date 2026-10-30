# AML-08 worker acceptance

Validated on **2026-10-30**. This closes the bounded continuous-worker stage;
it does not establish mainnet capacity, whole-chain coverage or five-VM readiness.

## Environment and unchanged contracts

- Cached Rust 1.92 Docker toolchain, offline locked Cargo dependencies.
- Actual disposable ClickHouse **23.8.16.16**, **2 GiB / 2 CPUs**, isolated
  Docker network and synthetic credentials/data. No real blockchain RPC used.
- Shared schema **v5**, **39 tables / 53 views / 682 physical table columns**,
  checksum `8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`.
  No schema, other chain adapter or risk-policy weight change.
- Operational cursor/queue stored on a persistent local filesystem; analytical
  data uses existing Bitcoin VM ClickHouse tables. Central storage is unchanged.

## Executed checks

Evidence is generated under ignored `test-results/aml08/`.

| Check | Result | Evidence |
|---|---|---|
| Ordinary Bitcoin Rust tests | 70 passed | `bitcoin-tests.log` |
| Real ClickHouse worker acceptance | 1 passed, 23 successful jobs, 411.37 seconds | `worker-tests.log`, `worker-evidence.json` |
| Actual non-root CLI process and fault tests | Passed | `native-cli.log`, `native-cli-receipt.json` |
| Central ordinary tests | 7 passed | `central-unit.log` |
| Central acceptance with actual adapter/worker JSON | 3 passed | `central-acceptance.log`, `central-evidence.json` |
| Repository Python suite | 107 passed, including 5 real Bash VM-control tests | `python-tests.log`, `vm-launcher-tests.log` |
| Resolved Compose configuration | Passed | `compose-config.json`, `compose-check.log` |
| JavaScript and Bash syntax | Passed | Actual checked scripts/JS |
| Actual browser UI and saved investigation | Passed | `ui-receipt.json`, `worker-ui.jpg` |

The ordinary Bitcoin run includes 41 library tests, 11 RPC/ingestion-control,
16 probe and 2 mapping tests. Twelve separate ingestion-resilience cases and
the older standalone database fixtures are ignored in that ordinary run and
were **not** rerun as AML-08 acceptance. Central acceptance also retains the
existing AML-02/AML-07 payload regressions; those are previously generated real
ClickHouse fixtures, not fresh executions of every earlier DB test.

The dictionary was regenerated with worker ownership anchors and checked for
stale output. No new warehouse object was introduced. The new protected API path
was added to OpenAPI; a full OpenAPI schema validator was not available locally.
The entire legacy `linux-cli.sh` suite was not rerun because jq was absent and
the official helper download failed. Its modified Bitcoin startup/control
behavior was instead executed through five actual Bash tests against an
isolated Docker command recorder; they are part of the passing Python suite.

## What the acceptance proves

1. Before the first block the worker waits without invented jobs/results.
2. Canonical address discovery uses a fixed height and lexicographic page
   cursor. Six synthetic addresses span several pages. Restart after the first
   successful job retains the cursor and queued addresses without rediscovery
   of that completed job in the same sweep.
3. Activity fingerprint and consolidation evidence reach the persisted risk
   assessment inputs. Cluster memberships remain candidate-only with null
   entities and no active ownership memberships. A pending seed stays pending;
   the worker never imports, approves or reviews it.
4. Following an **explicit fixture-operator** approval, the next sweep sees
   valid exposure. Following explicit revocation, the old assessment becomes
   stale immediately via its existing dependency checks; the next sweep computes
   UNKNOWN. Six jobs complete per sweep: 18 before the rollback test.
5. Rollback from height 1 to genesis resets discovery to the current branch.
   Five more jobs complete (23 total), and old source-bound assessments stay
   historical. The source/status endpoint is service-key protected.
6. Actual CLI containers run as UID **10001**, with a read-only root, one CPU,
   512 MiB memory and shared persistent checkpoint/lock directories. A competing
   worker is rejected. Killing a running job with SIGKILL, then starting a new
   container, resumes it at attempt 2 and completes it. SIGTERM checkpoints a
   stopped state, which the liveness command correctly rejects.
7. The real CLI with a one-second job deadline attempts two different jobs;
   both enter retry independently and `once` returns exit code 2. Unit tests
   cover delayed retries, priority of new due jobs, exhausted attempt budgets,
   manual retry, config changes, checksum corruption and stale/backward-clock
   heartbeat detection.
8. The central workflow preserves the entire captured worker status through
   create, Export and reopen. The real HTML/JS renders the completed count,
   heartbeat **at capture**, bounded-scope warning and UNKNOWN risk. Browser
   requests use a local fixture server; central Neo4j persistence is a test
   double. No live Neo4j acceptance is claimed.

The first DB test attempt expected a scored exposure from a synthetic label
whose entity type was `exchange`. The existing service-boundary rule correctly
kept that path as context/UNKNOWN. Only the synthetic fixture type was changed
to a non-service test label to test reviewed-seed scoring/revocation; risk logic
and weights were preserved. The failed assertion log is retained separately.

## Practical limits

This is a serial, resource-bounded address-sweep worker, not an event-stream
processor or a whole-chain SLA. Addresses added before the cursor or beyond the
discovery height wait for a later sweep. Refresh interval starts after the
current pass drains. Failed jobs retain queue slots and require operator retry;
a full queue reports backpressure rather than silently dropping work.

The operational checkpoint retains 16 recent successful receipts, not an
immutable event log of every scheduling decision. A crash can leave sealed
sub-results or unsealed audit attempts, and retries use new run IDs. Existing
inspection/publication rules determine which analytical evidence is current.

The shared local kernel lock is for processes sharing one VM filesystem;
multi-host leases/HA are outside this stage. Real Core/mainnet history,
performance/false-positive evaluation, release deployment images, live Neo4j,
historical replay and the remaining DB/API parity stages still need acceptance.
Alerts, watchlists and organizational case management were not added here.

See [operation and limits](ANALYTICS.md) and [remaining tasks](../TODO_BITCOIN.md).
