# Bitcoin semantic evidence — AML-06

The adapter describes transaction shapes using `bitcoin_semantics_v1`. These
are investigation leads, not laundering labels, common ownership, identified
change or proof that particular satoshis moved between wallets. **Every event is
`score_eligible: false`**. Pattern-only investigations retain `UNKNOWN` / null
risk. The existing shared risk policy and its weights are unchanged.

## Versioned rules

| Candidate | Exact v1 predicate | Confidence |
|---|---|---|
| Consolidation | At least 5 inputs; exactly 1 positive output | 0.60 |
| Fan-out | 1–2 inputs; at least 5 positive outputs to at least 3 distinct address/script subjects | 0.60 |
| Possible CoinJoin | At least 3 distinct input subjects; at least 3 equal positive outputs to distinct addresses | 0.35 |
| Peel step | 1 input; 2 positive outputs; larger/smaller integer ratio at least 4; larger output below input value | 0.45 |
| Peel chain | 3–5 peel steps linked by actual spending of the previous step's largest output, as the next transaction's sole input | 0.50 |

Zero outputs are retained in evidence but excluded from positive-output counts.
Coinbase is ignored. Equal-output groups are selected by largest distinct
address count, then largest value; repeated payment to one address or bare
scripts does not satisfy the CoinJoin output predicate. Script subjects are
retained and never assigned to one participant. Input subjects also do not
identify people or establish that participants are independent.

These thresholds and confidence values are project heuristics, not calibrated
probabilities, a regulatory standard, or a published detector's accuracy claim.
Batch payments, wallet maintenance and collaborative transactions can match.
Possible CoinJoin does not identify a mixer service; many CoinJoin forms will
not match. No bridge/mixer event is generated without separate protocol evidence.

For context, [BIP78](https://github.com/bitcoin/bips/blob/master/bip-0078.mediawiki)
documents collaborative transactions that undermine common-input/change
assumptions. [How to Peel a Million](https://arxiv.org/abs/2205.13882) discusses
peeling heuristics; this implementation does not reproduce its clustering/ML
method or claim its results.

## Storage and historical data

`src/semantics.rs` is the detector and reader. `src/bitcoin/transactions.rs`
provides the canonical full-transaction validator shared with exposure.

- `db/warehouse.rs::map_evidence` produces single-transaction rows in the existing
  **`aml_semantic_aml_events`** table. The writer verifies every column and amount
  before recording the same block's expected counts and final publication fence.
  `aml_ingested_blocks.raw_evidence_json.semantic_detection` records the version,
  definition hash, processed/skipped counts and up to 25 skipped transaction IDs.
- Native event `subject_id` is a **transaction occurrence**, `tx:block_hash:tx_hash`.
  Whole-transaction input/output totals are exact satoshi strings and are never
  represented as amounts attributable to the investigated wallet.
- The reader re-derives matching events from validated canonical transaction
  witnesses and verifies stored evidence. `storage: canonical_fact` means that
  exact event was written with its block. A conflicting same-version row fails
  the query; it is not silently replaced or accepted.
- For older blocks without semantic facts, the reader computes evidence with
  `storage: computed_from_canonical_facts`. It does **not** append facts into an
  already sealed block or change old expected counts. Dedicated historical fact
  reprocessing remains **ING-04**. Updated legacy backfill naturally generates
  events when it publishes previously unmigrated blocks.
- Peel chains are assembled at investigation time, never inserted post-hoc into
  a block. Actual previous occurrence, outpoint index, value, subject and causal
  `(block, transaction_index)` must agree. Repeated txids are resolved using the
  same BIP30 occurrence guard as exposure. At most 2 steps are followed backward,
  then forward to 5 total. The largest output is not called change. Overlapping
  chains are descriptive, deduplicated by event ID, and never additive risk.

Investigation captures all returned events and detector/coverage metadata in
existing `aml_wallet_risk_assessments.evidence_json.inputs`. The central Neo4j
snapshot preserves the full response. A GET of semantic evidence alone writes
neither semantic facts nor risk assessments. No schema migration, shared server,
new run table, dependency, background worker or RPC call is introduced. Schema
**v5 / 39 tables / 53 views / 682 columns** remains unchanged; each network VM
keeps its own ClickHouse database and volume.

## API, scope and freshness

Protected by `X-AML-Service-Key`, shared query slots, timeout and `no-store`:

```text
GET /api/bitcoin/semantics/config
GET /api/bitcoin/wallet/{address}/semantics
GET /api/bitcoin/wallet/{address}/investigation
```

The semantic endpoint accepts [activity options](ACTIVITY.md). Roots are the
first **25 recent transaction occurrences** in that activity scope. Chain
context may extend outside its block/time/direction filters; this is explicit
in `semantic_coverage` and the UI. This is not a full-chain/paginated pattern
search. An empty result never proves absence of these patterns.

Caps: **60 cached transaction contexts, 4096 total legs, 256 legs per
transaction, 128 events, 2 MiB event evidence, 5 steps per chain**. Large
transactions are skipped rather than classified from incomplete legs. Root,
context, evidence and missing-prevout limitations produce explicit partial
coverage. Reaching the chain cap describes a bounded segment, not the full chain.

Events contain deterministic IDs, detector version/definition hash, confidence,
ordered inputs/outputs, source commit/revision/block, relationship IDs, exact
outpoint references, string satoshis and explicit limitations. `semantic_snapshot`
binds roots, events, coverage and source. Detector metadata in `fingerprint`
binds it to the existing risk evidence hash without changing the policy version.

The central consumer verifies those bindings and queries the cheap static config
endpoint before treating a saved snapshot as current. Changed version/hash marks
it stale; missing/unavailable config marks it unverified. Original semantic
evidence remains available as history; current risk is `UNKNOWN`. Existing
pre-AML-06 snapshots without semantic metadata retain their earlier contract.
Change the **detector version and definition** when changing rule meaning; never
reuse a version for conflicting canonical event bodies. Detector changes require
historical fallback/reprocessing planning rather than rewriting published rows.

The Bitcoin panel shows candidate, confidence, storage source, transaction/outpoint
details and scope. Export saves the central snapshot; reopening restores its
evidence. This is distinct from exporting a complete downloadable case file.

[Validation](SEMANTICS_VALIDATION.md) · [Remaining work](../TODO_BITCOIN.md)
