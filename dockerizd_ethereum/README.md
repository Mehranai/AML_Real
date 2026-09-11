# Ethereum AML Platform

> Current deployment uses only the main VM's Neo4j. Chain APIs read ClickHouse;
> temporary snapshots, Export and the non-ML evidence policy run centrally.
> See [Central investigations](../docs/CENTRAL_INVESTIGATIONS_FA.md).
> Older local Neo4j projection instructions below describe legacy tools, not the current UI workflow.

> New location: `AML_Whole/dockerizd_ethereum`. For the shared TRON/Ethereum
> entry page and Docker launcher, use [AML Whole](../README.md).

Standalone Ethereum evidence, graph, and analyst service. ClickHouse is the
authoritative warehouse. Neo4j is a rebuildable query projection used by the
investigation UI and source-to-target path search.

## Architecture

~~~text
Trace-capable Ethereum RPC / local Reth
                  |
                  v
        ethereum-ingestion
                  |
        +---------+----------+
        |                    |
        v                    v
transactions/logs     canonical value-flow edges
receipts/traces       semantic AML events
        |                    |
        +---------+----------+
                  v
          ClickHouse ethereum_aml
                  |
        +---------+-------------------+
        |         |                   |
        v         v                   v
 token metadata  entity intelligence  clustering/exposure workers
        |         |                   |
        +---------+-------------------+
                  v
       versioned evidence-risk policy
                  |
                  v
          ethereum-api + analyst UI
                  |
                  v
        Neo4j Wallet/TRANSFER projection
~~~

Neo4j is not a second blockchain warehouse. Wallets and transfer edges are
projected on demand from address_relationships_canonical and can be rebuilt.

## Implemented capabilities

- Finalized historical and continuous ingestion with durable checkpoints.
- Replay-safe completion markers and receipt/trace coverage flags.
- Native external ETH and recursive internal EVM transfers from
  debug_traceBlockByNumber using callTracer.
- ERC-20, ERC-721, ERC-1155 TransferSingle/TransferBatch, mint, and burn edges.
- Versioned swap, Optimism Standard Bridge, Tornado Cash mixer, and Uniswap
  liquidity event producers.
- Protocol-contract registry with detector versions and evidence references.
- Token metadata discovery queue and continuous metadata worker.
- Reviewed entity-label import with source, reviewer, confidence, role, and case
  evidence.
- Reviewed-entity clustering. Contract deployment is stored as a control claim,
  not silently promoted to common ownership.
- Explainable bidirectional exposure propagation with hop, amount share, time
  decay, service boundaries, path addresses, relationship IDs, and tx hashes.
- Evidence-policy risk scoring with persisted signals and assessments.
- Unified wallet investigation endpoint and UI.
- Source-to-target path search up to 10 hops with Neo4j projection.

## Risk output: no ML

This build intentionally does not train or deploy an ML model. The replacement
is ethereum_evidence_policy_v1, a versioned expert/evidence policy stored in
ClickHouse.

It evaluates approved illicit seeds, direct and indirect exposure, confirmed
mixer activity, mixer/bridge and swap/bridge sequences, and rapid same-asset
pass-through. Every contribution has evidence references and is persisted in
wallet_risk_signals.

The result is a risk_score from 0 to 100 and a risk level. It is not a
calibrated probability and is not a legal conclusion. The API explicitly
returns probability_claimed: false.

## First Docker start

~~~bash
cd "$HOME/AML_Whole/dockerizd_ethereum"
cp -n .env.example .env
~~~

Set ETH_RPC_URL, CLICKHOUSE_PASSWORD, and NEO4J_PASSWORD, then run:

~~~bash
docker compose up -d --build
docker compose ps
~~~

Schema migrations run through the one-shot ethereum-schema service. The
long-running services are:

- ethereum-ingestion: finalized block ingestion and resume.
- ethereum-token-metadata: token metadata queue worker.
- ethereum-analytics: daily clustering and exposure propagation.
- ethereum-api: analyst UI, investigation, path search, and Neo4j projection.

## Trace mode

ETH_TRACE_MODE accepts:

- required: fail ingestion when trace evidence is unavailable. Use this with a
  trace-enabled local Reth production node.
- auto: ingest receipts/logs when an external RPC does not expose debug trace
  APIs, and mark trace_data_complete = 0.
- disabled: do not request traces.

For production Reth parity use ETH_TRACE_MODE=required.

## Ports

| Component | Host address |
|---|---|
| Analyst UI and API | http://127.0.0.1:5001 |
| API readiness | http://127.0.0.1:5001/ready |
| ClickHouse HTTP | 127.0.0.1:28123 |
| ClickHouse native | 127.0.0.1:29000 |
| Neo4j Browser | http://127.0.0.1:28474 |
| Neo4j Bolt | 127.0.0.1:27687 |

## APIs

~~~http
GET /api/ethereum/wallet/{address}/investigation?limit=750
GET /api/ethereum/wallet/{source}/paths/{target}?max_hops=10&direction=outbound
GET /status
GET /health
GET /ready
~~~

The wallet response includes fingerprint, asset flows, counterparties, semantic
events, entity context, cluster memberships, exposure paths, evidence risk,
ClickHouse coverage, and Neo4j projection counts.

## Ingestion operations

~~~bash
docker compose run --rm ethereum-ingestion ethereum_ingestor probe
docker compose run --rm ethereum-ingestion ethereum_ingestor range --from-block 25736898 --to-block 25736900
docker compose logs -f ethereum-ingestion
~~~

## Entity-label CSV

~~~csv
address,entity_id,entity_name,entity_type,address_role,confidence,risk_level,is_exposure_seed,seed_category,source_record_id,evidence_refs,review_status,reviewed_by,review_reason
0x1111111111111111111111111111111111111111,eth:scam:case1,Example Scam,scam,UNKNOWN,1.0,90,true,SCAM,case-row-1,case:123|https://evidence.example,APPROVED,analyst-a,Confirmed from case evidence
~~~

A high risk label is not automatically an exposure seed. Propagation starts only
when is_exposure_seed=true, the row is approved, risk is positive, and
seed_category is present.

Import the CSV after mounting it into the container:

~~~bash
docker compose run --rm ethereum-schema ethereum_ingest_entity_labels --file /data/ethereum_labels.csv --source-id internal_cases --source-name "Internal reviewed cases" --source-type INTERNAL --trust-tier VERIFIED --created-by analyst-a --submitted-by analyst-a
~~~

## Analytics operations

~~~bash
docker compose run --rm ethereum-schema ethereum_discover_address_clusters
docker compose run --rm ethereum-schema ethereum_propagate_exposure --max-hops 5 --hop-decay 0.65 --time-half-life-days 365 --max-paths-per-subject 3
docker compose run --rm ethereum-schema ethereum_assess_wallet --address 0x1111111111111111111111111111111111111111
~~~

## Storage lifecycle

Canonical chain evidence is retained. Recomputable exposure paths have a
180-day TTL. Risk signals are retained for two years and assessments for five
years. These windows prevent analytical snapshots from growing without bound
while preserving the canonical facts needed to reproduce them.

Named volumes survive docker compose down. Do not add --volumes unless the
Ethereum ClickHouse and Neo4j data should be deleted intentionally.

## Validation

~~~bash
cargo fmt --all -- --check
cargo test --all-targets
docker compose config --quiet
~~~
