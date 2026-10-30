# Bitcoin clustering leads

AML-02 adds **investigative candidates**, not an ownership database. It uses the
existing schema v5 in the Bitcoin VM's independent ClickHouse. No schema,
central ClickHouse, other chain adapter or scoring weight is added.

## What is detected

- `common_input_candidate`: at least two distinct positive input addresses in
  one complete canonical transaction, with at most 16 distinct members.
- `change_candidate`: a deliberately narrow lead from one input and two positive
  outputs to different new addresses. Exactly one output must match the input's
  address encoding family. Both outputs with the same family, address reuse and
  ambiguous shapes produce no change lead. This does **not** identify change.
- At least two distinct input addresses plus two equal positive outputs to
  distinct addresses suppress leads as possible collaboration. This is a guard,
  not a complete CoinJoin or PayJoin classifier. Undetected collaboration remains
  possible; **every** candidate is uncertain.
- Coinbase, positive script-only output contexts, script-only/zero inputs and
  overlarge member groups do not acquire invented address memberships.

There is no transitive union of addresses. A transaction-specific group is not
merged into another group. Confidence is a versioned heuristic match strength,
not the probability of ownership or crime. `same_entity` and `entity_id` remain
null. `ownership_claimed`, `score_eligible` and `is_exposure_seed` remain false,
including after a human accepts a lead.

The protected `/api/bitcoin/clusters/config` endpoint publishes the compiled
version, definition hash, limits and limitations. The initial version is
`bitcoin_cluster_leads_v1`; changing the definition invalidates old current use.

## Scope and stored evidence

Roots follow the selected activity height, UTC time and direction filters.
At most 25 recent transactions, 256 legs per transaction, 4096 total legs and
512 KiB of generated candidate evidence are examined. Complete transactions are
read through canonical views and checked by the existing transaction reader;
partial legs are never used to generate a lead. Evidence preserves the exact
block/transaction occurrence, commit/revision, input outpoints, leg identifiers,
integer satoshi strings, output positions and the matching rule.

Every run uses a fresh ID and stores:

| Existing table | Contents |
|---|---|
| `aml_cluster_runs` | Source snapshot, bounded scope, detector, complete/partial status and exact response frame |
| `aml_cluster_claims` | One version-specific lead claim for each candidate member, with source, confidence and witnesses |
| `aml_cluster_memberships` | Version-specific `candidate_only` membership; never an active entity membership |
| `aml_cluster_versions` | Candidate version, supporting claims and review history with predecessor references |
| `aml_analysis_publications` | Existing shared publication seal after complete output counts and source-fence checks |

Complete **bounded** runs are sealed. This never asserts a complete search of
the chain. A capped run stores unsealed audit rows with `status=partial`,
`storage=unsealed_audit_only` and `persisted=false` (no complete publication).
It cannot be reviewed or treated as a current published cluster. Inspection
checks the entire expected output set, including raw audit rows and counts.
Before the first committed block, the response is `waiting_for_blocks`, with
no run or publication.

`cluster_snapshot.current` requires a valid complete seal. `source_current`
separately says that the framed evidence, detector and reviews still match the
source. An explicitly partial audit snapshot can remain source-current without
being a published cluster. This prevents optional partial leads from disabling
otherwise valid, independently verified risk evidence. A missing seal for a run
that claims to be complete still fails closed.

## Review and history

Reviews require an explicit trusted internal caller, actor, reason, references
and the exact current `run_id`/`expected_version`. The service key authorizes the
internal operation; `reviewed_by` is an audit field, not a separate user-login
system. Do not expose this mutation endpoint directly to an unauthenticated
public client. Automated imports and workers never approve candidates.

`approved` means **accepted investigative lead only**. Memberships remain
pending and no entity, intelligence claim, risk seed or risk signal is created.
`rejected` makes memberships inactive; `revoked` records revocation. Decisions
create fresh immutable runs/versions. Old decisions and evidence remain readable.
A stale version returns conflict instead of overwriting a newer review. A
separate kernel review lock coordinates API/CLI reviewers without taking the
long-running ingestion owner's lock. All reviewer processes on the VM must use
the same shared `BITCOIN_WRITER_LOCK_DIR`; this is not a distributed lease.

Review decisions are bound to the exact source snapshot and detector definition.
Within that snapshot, recomputing the same candidate preserves its latest valid
review. A changed chain revision, reorg or detector definition makes the old
version historical and requires fresh computation/review. Historical acceptance
is retained; it is not silently promoted to an approval on new source data.

## API and CLI

All endpoints below require `X-AML-Service-Key`:

```text
GET  /api/bitcoin/clusters/config
GET  /api/bitcoin/wallet/ADDRESS/clusters?limit=25&from_block=HEIGHT
GET  /api/bitcoin/wallet/ADDRESS/clusters/RUN_ID
GET  /api/bitcoin/wallet/ADDRESS/clusters/RUN_ID/snapshot
POST /api/bitcoin/wallet/ADDRESS/clusters/review
```

The collection GET computes/persists a bounded run, like the exposure workflow;
run inspection and snapshot GETs do not recompute or write a review.

```bash
bitcoin_aml clusters --help
bitcoin_aml clusters ADDRESS
bitcoin_aml clusters inspect ADDRESS RUN_ID
bitcoin_aml clusters review ADDRESS review.json
bitcoin_aml clusters config
```

Copy the actual IDs from the current response into a separate review file:

```json
{
  "network_id": "bip122:000000000019d6689c085ae165831e93",
  "run_id": "RUN_ID_FROM_RESPONSE",
  "cluster_id": "CLUSTER_ID_FROM_RESPONSE",
  "expected_version": "SAME_RUN_ID_FROM_RESPONSE",
  "decision": "approved",
  "reviewed_by": "reviewer-name",
  "reason": "Why this lead is useful and what was checked",
  "evidence_refs": ["case:your-reference"]
}
```

The IDs in this example are placeholders. Unknown fields, imported ownership,
invalid networks/addresses, empty actor/reason and missing evidence are rejected.
No Bitcoin RPC or private wallet credentials are accessed by these commands.

## Investigation and central snapshots

`clusters`, `cluster_coverage` and `cluster_snapshot` are included in the address
investigation. The panel displays member candidates, version, confidence,
transaction/outpoint evidence and review provenance. It does not draw new
ownership edges or add candidate members to a global entity graph.

The main VM preserves the full lead set in its existing Neo4j investigation
payload. Create, Export and reopen verify the source, detector definition,
publication, exact content and latest review version. A changed review or source
makes a saved result historical; its old evidence is retained. Hashes check
content consistency and do not constitute digital signatures.

Continuous bounded refresh is available through the [AML-08 worker](ANALYTICS.md).
Global clustering, performance acceptance on a real node/VM and the remaining
four-network migration remain separate tasks.
See [validation](CLUSTERING_VALIDATION.md) for the tests actually executed.
