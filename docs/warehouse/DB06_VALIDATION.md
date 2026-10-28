# DB-06 validation — 2026-10-27

Historical stage report. The subsequent v5 visibility upgrade is documented in [DB-07](DB07_VALIDATION.md); v4 remains immutable.

DB-06 adds the `typed_relationships_v1` interpretation/model and canonical
relationship reader. The storage contract remains **v4**, with **38 tables,
40 views and 668 physical table columns**. No migration v5, applied schema
rewrite, production rollout or legacy-history conversion was performed.
The four adapters retain independent ClickHouse deployments on their own VMs.

## Verified scope

- Shared Rust: **22 non-ClickHouse tests** passed (17 existing, five new),
  plus **two new ClickHouse integration tests**. The integration tests exercise
  the same reader on all four network fixtures and separate repeated-txid
  Bitcoin occurrences. Other stages' 12 ignored shared ClickHouse tests were
  not rerun here; their results remain in DB-04/DB-05 reports.
- Bitcoin: **53 tests** passed — 10 library, two ClickHouse pipeline,
  23 ingestion resilience, 16 probe and two warehouse mapping tests.
  The mapping test also rejects an address inconsistent with its script.
- Analytical Node: **19 unit tests** passed, including typed Bitcoin
  normalization, NULL addresses for transaction/script nodes, exact one-satoshi
  evidence and rejection of pairwise/continuity claims. No live Neo4j test or
  deployed snapshot migration was performed; the storage Cypher is unchanged.
- Both native ClickHouse SDK tests passed using the checked-in TRON and BSC
  integration-test sources in small dependency harnesses. They exercised the
  shared reader on empty neighborhoods and parsed the witness SQL with
  `readonly=1`. This does not claim a full
  adapter test run or active writer/reader cutover for those networks.
- **67 Python tests** passed, including dictionary provenance and consistency;
  dictionary regeneration and offline `--check` passed.
- Strict Clippy (`--all-targets -- -D warnings`) passed for the shared crate,
  Bitcoin and Analytical Node. Sources were formatted with rustfmt.
- Both browser smoke cases passed in disposable-container Chromium
  **156.0.8078.47**: legacy snapshots and typed transaction/script nodes with
  NULL addresses. Screenshots were inspected. Exact BTC amounts, evidence,
  partial-history notice, graph counts and UNKNOWN risk were checked.
  Host Edge returned an empty headless DOM; it is not counted as a passing
  browser. The successful Chromium cases use isolated fixture profiles.
- `export_sql --relationships --check` matched the review artifact, and
  `--contract-json --check` matched the unchanged effective v4 definition.
  The Bitcoin embedded-SQL unit test also checks the relationship template.

## Evidence exercised

1. Two inputs and three outputs create five observed legs and a typed graph,
   without allocating input values to outputs or claiming common ownership.
   Addressless scripts, exact satoshis and a one-satoshi fee survive mapping.
2. Account-network transfers retain address endpoints, asset identity, exact
   UInt256 MAX values, ordered EVM trace positions and NFT identifiers.
   The latter trace/NFT assertions are model tests; they are not detector tests.
3. Every Bitcoin graph leg must match the canonical vin/vout fact on network,
   block height/hash/time, commit/revision, transaction hash/index, leg index,
   amount, subject, nullable address and outpoint. A missing witness and an
   existing input with a conflicting amount both cause an error.
4. Unpublished facts remain invisible. Transaction selection includes the
   complete occurrence key; the same txid in two blocks produces distinct
   transaction nodes, selected context and edge identities. The fixture tests
   occurrence separation, not consensus validation of historical BIP30 blocks.
5. Transaction/edge caps signal truncation. An observed publication-state
   change during reading causes an error; the moving-state connection simulates
   this race. No distributed-snapshot or multi-writer lease claim is made.
6. Mixed-network graph merges, invalid database identifiers, out-of-range
   limits, inconsistent endpoint types, fabricated output indices/outpoint
   references and Bitcoin pairwise transfers are rejected.
7. The Bitcoin pipeline again covered visibility, restart, holdings, paths,
   reorg and incomplete legacy migration. Path traversal now preserves forward
   `(height, transaction_index)` order and reports possible connectivity only.

Fixtures ran on disposable ClickHouse **23.8.16.16**, using fixture credentials
only. [The acceptance receipt](db06-acceptance.json) records migration histories,
identity, normalized DDL/column hashes and the relationship SQL digest for four
independent `db06_graph_*` databases. All four have identical schema hashes,
38 tables, 40 views and 668 table columns. Adversarial witness rows intentionally
remain in the disposable Bitcoin fixture until container cleanup.

## Unchanged identities and limits

- v1: `147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`
- v2: `c76d93f7c782fc09b7a87d5b7a0337203b595ca60fcd3fc6ca85092c2bb9b5f5`
- v3: `9548e226c0d1d4259fe488bab1d9a137232e6e68d311054190b0034db7e0800e`
- v4: `955e0c5ff2e6bb7000793e66408312f4d0172d026f15f328a9431ee050b568ec`

The active Bitcoin writer validates typed relationships and script/address
agreement before publication; its API uses the common reader. The other three
networks' active legacy writers/readers still await DB-08. Existing inconsistent
history is retained and rejected at the interpreted graph boundary rather than
silently repaired. Bitcoin shared-model address checks cover syntax/mainnet
prefix only; full checksum/network and script-derived identity checks belong
to the Bitcoin adapter. The deployed script digest remains SHA256 of canonical
hex text; it is not a consensus script hash.

DB-07 remains responsible for the full canonical/visibility/invalidation
contract, including dependent analytics, aggregates, caches and reorg handling.
DB-08 history cutover/restore, DB-09 operations, ING-04 and Bitcoin AML workers
remain open. No claim of full AML feature parity or deployed VM acceptance is
made. Local environment files and Git history were unchanged. Only task-owned
test containers were removed after verification; existing network/build caches
were retained.
