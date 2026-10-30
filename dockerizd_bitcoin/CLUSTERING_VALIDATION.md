# AML-02 — clustering lead acceptance

Validated on **2026-10-30**. This completes the bounded, on-demand candidate
stage. It does not complete continuous workers, global address clustering,
four-network legacy migration or production acceptance.

## Environment and schema

- Rust 1.92 in the cached `public.ecr.aws/docker/library/rust:1.92-slim-bookworm`
  image, with `cargo test --offline --locked` and the local Cargo cache.
- Actual disposable ClickHouse **23.8.16.16**, limited to **2 GiB / 2 CPUs**,
  isolated Docker network and synthetic credentials. No production database,
  blockchain RPC credentials or private wallet was used.
- Shared schema **v5**, **39 tables / 53 views / 682 physical table columns**,
  checksum `8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`.
  No DDL/migration or scoring-weight change was needed for AML-02.
- Each network retains its independent ClickHouse/database on its own VM.
  Clustering uses the four existing `aml_cluster_*` tables and the existing
  analysis publication ledger in Bitcoin's database.

## Executed checks

| Check | Passed | Evidence under ignored `test-results/aml02/` |
|---|---:|---|
| Ordinary Bitcoin Rust tests | 66 | `bitcoin-tests.log` |
| New real ClickHouse clustering acceptance | 1 | `clustering-tests.log`, `cluster-evidence.json` |
| Related real ClickHouse pipeline, activity, exposure, risk, intelligence and semantic regressions | 8 | `related-db-tests.log`, `remaining-db-tests.log` |
| Central ordinary Rust tests | 7 | `central-final-unit.log` |
| Central acceptance using actual adapter JSON | 3 | `central-final-acceptance.log`, `central-evidence.json` |
| Python repository tests | 102 | `python-tests.log` |
| Real HTML/JS in the in-app Chromium browser | Passed | `ui-receipt.json`, `clusters-ui.jpg` |
| Actual CLI help without DB/RPC configuration | Passed | `cli-help.log` |

Central acceptance was repeated with the newly generated AML-02 exposure,
activity, intelligence and semantic payloads. The unchanged holdings regression
also consumes the prior AML-07 real ClickHouse payload; the two expensive
holdings database cases and the twelve ingestion-resilience cases were **not**
rerun in this stage. Ignored cases in the ordinary Cargo run are not counted
as passed. JavaScript syntax and the working diff whitespace were checked.
The data dictionary was regenerated with the active clustering ownership paths
and checked for stale generated output; its focused regression suite also passed.
The API contract was updated and reviewed; a full OpenAPI schema validator was
not available in this offline test environment.

The browser serves actual generated adapter/central acceptance payloads through
a local synthetic HTTP fixture. It checks accepted-lead wording, candidate
members, reviewer/reason/reference, expanded exact 600/400-satoshi input
witnesses, UNKNOWN risk, Export and reopening the saved investigation. The
browser transport and central Neo4j persistence are mocked in these fixtures;
this is **not** an acceptance test against a running Neo4j deployment.

## What the new acceptance demonstrates

- Common-input and narrowly defined change leads use complete canonical
  transaction occurrences. The equal-output collaboration guard suppresses
  leads. Coinbase, ambiguous change, script-only contexts, duplicate members,
  transaction limits and evidence provenance have explicit coverage.
- Before the first block, computation reports waiting rather than inventing a
  cluster. Root/resource caps produce explicit partial coverage; partial rows
  remain unsealed audit evidence and cannot be reviewed.
- Runs, per-member claims, candidate memberships and versions persist in the
  existing tables. Source, detector/config and exact evidence hashes bind the
  full output set. Missing publication or altered child rows fail verification.
- Approved, revoked and rejected decisions create immutable versions and retain
  actor, reason, references and predecessors. History survives `OPTIMIZE FINAL`.
  Stale review requests conflict; malformed and unauthorized requests are
  rejected. Recomputing on the same snapshot retains the latest valid review.
- Approval never creates active ownership memberships, entity IDs, intelligence
  claims, exposure seeds or risk inputs. A source revision/reorg makes the old
  result historical. Its prior evidence is retained, not promoted to current.
- Central create, Export and reopen preserve pending/approved/revoked/change
  payloads. Changed members, review, witnesses, detector configuration or source
  invalidate current use. Fresh explicitly partial audit output does not disable
  otherwise valid independent risk evidence; invalid complete output does.

## Failures encountered and resolved

An early review lookup repeated nested visibility views in one query and hit
the fixture's memory cap. It was replaced with bounded separate reads;
clustering inspection/lookup queries now cap threads, execution time and memory.
The complete clustering acceptance then passed on the same 2 GiB fixture in
132.02 seconds.

During the serial related regression run, intelligence schema initialization
encountered a connection reset. The container remained running without an OOM
termination. Only its disposable intelligence fixture database was reset, and
the intelligence and semantic tests passed on retry. The failed attempt remains
in the first log; the successful retry is in `remaining-db-tests.log`.

Python's default sandbox temporary directory rejected file creation. With
`TEMP`/`TMP` set to a writable directory under `tmp/`, all 102 tests passed.
Docker runs used the already authorized host Docker access; no Docker settings
or production containers were changed.

## Remaining limits

The heuristics are project definitions, not a calibrated ownership model or a
complete CoinJoin/PayJoin classifier. Confidence is not a crime probability.
No transitive/global address merging is performed. Every lead remains uncertain
after review. Reviews bind the exact source snapshot; any revision or detector
change requires fresh computation/review. The review lock coordinates processes
sharing the configured filesystem on one VM, not independent hosts.

Continuous workers/cursors/heartbeats are **AML-08**. Real Core/mainnet data,
concurrency and capacity under production load, release images, live Neo4j and
the five target VMs still require their own acceptance. No claim is made that
all AML capabilities or the four-network migration are complete.

See [behavior and commands](CLUSTERING.md) and the [remaining tasks](../TODO_BITCOIN.md).
