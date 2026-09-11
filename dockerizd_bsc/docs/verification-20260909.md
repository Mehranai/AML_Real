# BSC Investigation Verification - 2026-09-09

Scope: BSC investigation delivery to the shared AML Whole gateway and main-VM Neo4j.
This is an integration test record, not production capacity or security certification.

## Passed

- BSC Rust library: 42 unit tests; the five database tests are intentionally ignored in this run and executed separately with real ClickHouse.
- BSC strict Clippy: all targets, warnings denied. Rust formatting checked.
- Five real ClickHouse integration tests: schema/checksums, canonical replay, crash boundaries, reorg repair and dead-letter recovery.
- Investigation API against a disposable ClickHouse 23.8 instance: canonical revision isolation, fingerprints, bridge evidence columns, metadata worker, exact UInt256 holdings and NFT holdings.
- Path tests: ten hops, incoming/outgoing direction, chronological order, asset isolation, invalid-address rejection and explicit truncation.
- Intelligence tests: pending claims cannot seed exposure; approved claims can; a reviewed exchange boundary stops propagation; rejecting that claim removes the boundary.
- Linux BSC source Docker build with locked, offline vendored dependencies; private API tested inside a read-only container as UID 10001.
- BSC browser against real ClickHouse and central Neo4j 5.26: selection, wallet graph, non-ML evidence risk, live holdings, temporary snapshot, permanent Export, network-qualified graph nodes and immutable reopen.
- BSC canvas-pixel and layout checks at 390/768/1440 pixels, including resizing a loaded graph; screenshots inspected.
- Existing TRON/Ethereum browser regression: owner isolation, expiry cleanup, persistence after restart, idempotent Export, path snapshots, offline-chain isolation and responsive layouts.
- Gateway routing: five tests. Linux CLI contract tests and main/BSC Compose configuration validation.
- Shared Analytical Node: 13 unit tests and strict Clippy during this delivery.

## Reproduce

From the repository root, with Docker running, Rust 1.92, Node.js, installed npm dependencies and the three application images already built:

```bash
npm ci
npx playwright install chromium
(cd dockerizd_bsc && cargo test --locked --offline --lib)
(cd dockerizd_bsc && cargo clippy --locked --offline --all-targets -- -D warnings)
node --test gateway/tests/routing.test.mjs
bash scripts/tests/linux-cli.sh
node dockerizd_bsc/tests/investigation.mjs --docker-api --central --ingestion
node gateway/tests/browser.mjs
```

The offline Cargo commands require a populated dependency cache. On a clean connected build host,
run `cargo fetch --locked` in each Rust project first. Linux CLI tests also require Bash and jq.
Browser tests require Docker images `bsc-aml-service:local`, `aml-analytical-node:local`,
`aml-whole-gateway:local`, `clickhouse/clickhouse-server:23.8`, and `neo4j:5.26-community`.

The investigation test creates a uniquely named disposable ClickHouse container and, with
`--central`, an isolated main Compose project. With `--docker-api`, the BSC API itself runs in Linux
Docker; schema/import tools run from the host Rust build. Tests remove only their own containers,
networks and temporary Neo4j volume. Existing user chain databases are not used or erased.
Screenshots and generated JSONL fixtures go to ignored `test-results/`.

The database and graph engines are real. RPC responses, wallet identities, labels and event data
in these tests are controlled fixtures. No real person's wallet is labeled by this test data.

## Still Required Before Production

- Finalized RPC/trace coverage and balance reconciliation against the actual production BSC node.
- Historical throughput of at least twice the observed chain rate, a 24-hour soak and investigation query load tests.
- Vetted entity/protocol intelligence, advanced fingerprint ratios and any future automatic cross-chain bridge correlation.
- Backup/restore rehearsal, upgrade/rollback drill, monitoring/alerts, TLS/access controls and security/dependency audit.

Holdings are a separate live read, not part of the immutable exported investigation.
Missing history or missing labels never establish that a wallet is clean.
The shared risk score is evidence-based, without ML, and is not a calibrated laundering probability.
