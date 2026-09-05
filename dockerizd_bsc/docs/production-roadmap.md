# BSC Production Roadmap

این فایل تنها checklist مرجع BSC است. تیک هر مرحله فقط بعد از pass شدن Definition of Done همان مرحله زده می‌شود.

## Progress

- [x] Phase 0 - Architecture, scope and production gates
- [x] Phase 1 - Rust foundation and validated BSC configuration
- [x] Phase 2 - Minimal ClickHouse schema and automatic migrations
- [x] Phase 3 - Canonical block, transaction, receipt and log ingestion
- [x] Phase 4 - Transfer extraction and internal traces
- [ ] Phase 5 - Reliable historical/follow sync, replay and reorg handling (capacity gate only)
- [x] Phase 6 - Semantic AML event producers and protocol registry
- [ ] Phase 7 - Token metadata, holdings and wallet fingerprint
- [ ] Phase 8 - Entity intelligence and reviewed clustering
- [ ] Phase 9 - Neo4j graph, 10-hop paths and exposure propagation
- [ ] Phase 10 - Unified investigation API and BSC analyst UI
- [ ] Phase 11 - AML Whole gateway integration
- [ ] Phase 12 - Production hardening, load validation and release gate

## Phase 0 - Architecture, scope and production gates

Status: **Complete**

- [x] Confirm mainnet chain id `56`, network id `eip155:56` and native BNB identity.
- [x] Select Ethereum/EVM ingestion as the technical base and TRON investigation output as the parity target.
- [x] Define ClickHouse as source of truth and Neo4j as rebuildable projection.
- [x] Define finality, reorg, idempotency, data minimization and bridge evidence boundaries.
- [x] Explicitly exclude unused ML/risk tables and premature gateway registration.
- [x] Define phase-by-phase acceptance criteria below.

Definition of Done: architecture and roadmap exist, no unconnected runtime code/schema is introduced, and BSC is not advertised as ready.

## Phase 1 - Rust foundation and validated BSC configuration

Status: **Complete**

- [x] Create independent `bsc_aml` Rust package with locked dependencies.
- [x] Add generic EVM identifiers supporting `eip155:56`, native BNB and token/NFT assets.
- [x] Add strict config with BSC-prefixed variables and no silent cross-chain defaults.
- [x] Add RPC capability probe for `eth_chainId`, `eth_getBlockByNumber(finalized)`, block receipts and debug trace.
- [x] Refuse startup when chain id is not 56 or mandatory production capabilities are absent.
- [x] Add structured error types, tracing without secrets and unit tests for all validation boundaries.
- [x] Add `cargo fmt`, `clippy -D warnings`, unit test and dependency audit commands.

Definition of Done: tests pass without ClickHouse/Neo4j; a fake Ethereum endpoint is rejected; BSC endpoint capabilities are reported accurately; no ingestion exists yet.

## Phase 2 - Minimal ClickHouse schema and automatic migrations

Status: **Complete**

- [x] Add immutable, checksummed migration runner executed automatically by Compose schema job.
- [x] Create only core tables with a producer/consumer/retention dictionary.
- [x] Add canonical views that filter by current block hash and deduplicate replayed rows.
- [x] Add bloom/set indexes only for established wallet, tx hash, block and event query patterns.
- [x] Configure codecs, partitions, TTL/retention only where evidence replay permits it.
- [x] Validate schema at service startup and fail on missing/changed columns.
- [x] Integration-test fresh install, restart, checksum drift and forward migration.

Initial core tables: `ingested_blocks`, `transactions`, `evm_logs`, `address_relationships`,
`transaction_features`, `semantic_aml_events`, `sync_state`, `ingestion_failures`, `ingestion_benchmarks`,
`token_metadata`, `token_metadata_discoveries`, `token_metadata_jobs`.

Definition of Done: clean ClickHouse starts through Docker with no manual cargo schema command; every column has a documented writer and reader; duplicate replay is invisible in canonical views.

## Phase 3 - Canonical ingestion

Status: **Complete**

- [x] Fetch finalized blocks with full transactions.
- [x] Batch-fetch or safely fan out receipts with bounded concurrency and retry policy.
- [x] Persist block/transaction/log evidence atomically at block-completeness level.
- [x] Decode fee, status, contract creation, calldata and event ordering correctly.
- [x] Distinguish no transactions/logs from unavailable data.
- [x] Record provider/client/version and coverage for reproducibility.
- [x] Add golden fixtures from selected real BSC blocks, including failed and contract-creation transactions.

Definition of Done: a fixed block range ingests twice with identical canonical counts and zero silent receipt/log gaps.

## Phase 4 - Transfers and traces

Status: **Complete**

- [x] Native BNB value transfers, including successful contract creation value.
- [x] ERC-20/BEP-20 transfer events with raw UInt256 amounts.
- [x] ERC-721 NFT transfers with full-width decimal token ids.
- [x] ERC-1155 single and bounded batch transfers with token id and quantity.
- [x] Internal native transfers from `callTracer`; value-moving CALL/CREATE/CREATE2/SELFDESTRUCT are retained and delegate/static/callcode are not misreported.
- [x] Zero-address mint/burn evidence is retained without treating the zero address as a normal wallet consumer.
- [x] Required trace coverage and explicit development degraded mode.
- [x] Revision-gated token discovery producer with cross-block deduplication.

Definition of Done: transfer edges reconcile with selected receipt/log/trace fixtures and every edge links to tx/block evidence.

## Phase 5 - Historical sync and reliability

- [x] Historical ranges, follow mode, auto-resume cursor and bounded batching.
- [x] Parent-hash continuity check and append-only canonical repair.
- [x] Replay CLI by range/hash and retryable dead-letter workflow.
- [x] Crash tests between each flush/cursor boundary.
- [x] RPC backoff, jitter, rate/concurrency control and endpoint failover rules.
- [x] Gap scanner and completeness repair worker.
- [x] Benchmark rows/sec, bytes/row and ClickHouse compression on representative BSC ranges.

Status: **Implementation complete; production capacity gate remains open.** پنج integration test روی ClickHouse
واقعی pass شده‌اند. benchmark بیست بلاک واقعی با RPC عمومی در بهترین اجرا `1.65 block/s`, `1453 row/s`,
`92.6 compressed bytes/row` و نسبت compression `7.93x` ثبت کرد؛ `live_rate_multiple=0.78` بود. تکرار gate
با local node یا RPC بدون throttling لازم است و نتیجه باید حداقل `2.0` باشد.

Definition of Done: forced crash/restart and simulated reorg produce no missing or duplicate canonical facts; sustained ingestion exceeds observed live chain rate by at least 2x in the test environment.

## Phase 6 - Semantic AML evidence

- [x] Versioned protocol contract registry with source and review status.
- [x] Swap and aggregator decoders, including AMM v2/v3 movement patterns.
- [x] Bridge producer with remote network/receiver/message evidence.
- [x] Liquidity add/remove, lending, staking and mint/burn producers.
- [x] Mixer/privacy and scam interaction evidence based on reviewed intelligence.
- [x] BSC system transaction classification separated from wallet behavior.
- [x] Confidence from evidence quality, never a money-laundering probability.

Status: **Complete.** رجیستری append-only است و فقط آخرین revision دارای `approved + enabled` وارد
classifier می‌شود. semantic factها پیش از marker کامل بلاک و با همان `block_state_revision` نوشته می‌شوند.
Unit fixtureهای مثبت و منفی و integration testهای replay/crash/reorg روی ClickHouse واقعی pass شده‌اند.

Definition of Done: each event type has positive/negative fixtures, provenance, version, deterministic id and no duplicate canonical event after replay.

## Phase 7 - Metadata, holdings and fingerprint

- [ ] Metadata discovery queue and bounded RPC worker for token name/symbol/decimals/standard.
- [ ] Manual metadata override/import with source and review fields.
- [ ] Compact wallet balance deltas and canonical holdings view, including native BNB.
- [ ] Fingerprint for flow direction, timing, churn, concentration, counterparties and semantic event ratios.
- [ ] Wallet activity trend and top incoming/outgoing counterparties.
- [ ] Data quality report for range, truncation, receipt, trace and metadata coverage.

Definition of Done: holdings reconcile against selected on-chain balances at the same finalized block; fingerprint uses only stored evidence and reports truncation honestly.

## Phase 8 - Entity intelligence and clustering

- [ ] Versioned intelligence source, raw claim, review decision and active label tables.
- [ ] Bulk import for known exchange, bridge, protocol, scam and sanctions data.
- [ ] Separate discovered cluster candidates from approved entity identities.
- [ ] Conservative clustering heuristics with evidence and false-positive controls.
- [ ] Analyst approve/reject workflow and immutable audit trail.
- [ ] BSC protocol/exchange seed pack kept as data, not hardcoded Rust logic.

Definition of Done: unreviewed claims cannot become active labels or exposure seeds; every active identity resolves to source and review evidence.

## Phase 9 - Graph, paths and exposure

- [ ] On-demand Neo4j projection from canonical ClickHouse relationships.
- [ ] Network-qualified uniqueness constraints and idempotent MERGE behavior.
- [ ] Wallet graph with depth/edge/time/operation filters.
- [ ] Directed source-to-target search up to 10 hops with hard safety caps.
- [ ] Amount/time/direction weighted exposure from reviewed seeds.
- [ ] Service-mediated boundary handling for exchanges, bridges and custodians.
- [ ] Persist best explainable paths and make runs versioned/reproducible.

Definition of Done: Neo4j can be erased and rebuilt; path output contains evidence edge IDs; truncation is explicit; direct and service-mediated exposure are distinguishable.

## Phase 10 - Investigation API and UI

- [ ] `/api/bsc/wallet/{address}/investigation` with graph, holdings, fingerprint, events, intelligence and quality.
- [ ] `/api/bsc/wallet/{source}/paths/{target}` with hop ceiling 10.
- [ ] health, readiness and ingestion status with dependency details.
- [ ] BSC UI with readable graph, independently scrollable evidence and path workflow.
- [ ] Loading, empty, partial, truncated and error states.
- [ ] Responsive and accessibility tests at mobile/tablet/desktop sizes.
- [ ] No AI risk percentage or legal conclusion.

Definition of Done: API contract tests and browser tests pass against real ClickHouse/Neo4j containers; a stored BSC address renders a nonblank graph.

## Phase 11 - AML Whole gateway

- [ ] Add BSC upstream and fixed Nginx route allowlist.
- [ ] Add BSC selector metadata and address validation.
- [ ] Copy the real BSC UI into the gateway image.
- [ ] Add status row, offline-chain isolation and cross-network navigation tests.
- [ ] Keep BSC hidden until its `/ready` contract and investigation tests pass.

Definition of Done: selecting BSC routes only to BSC; TRON/Ethereum behavior is unchanged; an offline BSC cannot break other networks.

## Phase 12 - Production release gate

- [ ] Non-root/read-only containers, dropped capabilities, resource limits and graceful shutdown.
- [ ] Private database ports, gateway authentication/authorization and TLS deployment guide.
- [ ] Metrics, dashboards and alerts for lag, gaps, retries, coverage, storage and latency.
- [ ] 24-hour soak, historical backfill benchmark and query load tests.
- [ ] Backup/restore ClickHouse test and clean Neo4j rebuild test.
- [ ] Schema/data compatibility upgrade and rollback rehearsal.
- [ ] Operator runbook, incident/replay procedure and data-retention policy.
- [ ] Security/dependency scan with reviewed exceptions.

Definition of Done: no known critical issue; zero unexplained data gap in the validated range; restart/replay/restore drills pass; operational owner approves release.

## ترتیب اجرای بعدی

capacity gate مرحله 5 هنوز باید روی node/RPC production با `live_rate_multiple >= 2.0` تکرار شود، اما
این محدودیت مانع correctness مرحله 6 نیست. مرحله توسعه بعد **Phase 7: metadata, holdings and fingerprint** است.
