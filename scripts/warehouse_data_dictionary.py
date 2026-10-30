#!/usr/bin/env python3
"""DB-01: reproducible schema dictionary and conservative, non-executable data map.

Normal runs are offline. --refresh creates only empty aml_inventory_* fixtures
on an explicitly disposable localhost ClickHouse. It never reads .env, changes
an adapter, migrates production rows, or treats a textual reference as a writer.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

from warehouse_inventory import CHAINS, ROOT, TestClickHouse, inventory, statements

OUTPUT = ROOT / "docs/warehouse/data-dictionary.json"
SNAPSHOT = ROOT / "docs/warehouse/db01-schema-snapshot.json"
CONTRACT = ROOT / "shared/warehouse/schema/0001_warehouse.json"
SQL = ROOT / "dockerizd_bitcoin/sql/migrations/20261022_0001_shared_warehouse.sql"
NETWORKS = {"tron": "tron:mainnet", "ethereum": "eip155:1", "bsc": "eip155:56",
            "bitcoin": "bip122:000000000019d6689c085ae165831e93"}

# These are reviewed migration destinations, NOT executable conversion rules.
# A missing destination is deliberately a blocker rather than an implicit drop.
DESTINATIONS = {
    "schema_migrations": (None, "schema", "retain_legacy_ledger"),
    "sync_state": ("aml_sync_state", "core", "rebuild_verified_checkpoint"),
    "ingested_blocks": ("aml_ingested_blocks", "core", "verified_block_publication"),
    "transactions": ("aml_transactions", "core", "verified_block_publication"),
    "address_relationships": ("aml_address_relationships", "core", "typed_relationships"),
    "transaction_features": ("aml_transaction_features", "semantic", "versioned_features_json"),
    "semantic_aml_events": ("aml_semantic_aml_events", "semantic", "verified_block_publication"),
    "evm_logs": ("aml_evm_logs", "core", "verified_block_publication"),
    "ingestion_failures": ("aml_ingestion_failures", "operations", "normalize_failure_state"),
    "ingestion_benchmarks": ("aml_ingestion_benchmarks", "operations", "preserve_metrics"),
    "token_metadata": ("aml_asset_metadata", "metadata", "asset_identity_and_review"),
    "token_metadata_jobs": ("aml_metadata_jobs", "metadata", "normalize_job_state"),
    "token_metadata_discoveries": (None, "metadata", "needs_discovery_evidence_contract"),
    "intelligence_sources": ("aml_intelligence_sources", "intelligence", "normalize_source"),
    "entity_labels": ("aml_intelligence_claims", "intelligence", "preserve_claim_and_review_history"),
    "intelligence_claims": ("aml_intelligence_claims", "intelligence", "preserve_claim_and_review_history"),
    "intelligence_reviews": ("aml_intelligence_reviews", "intelligence", "preserve_review_history"),
    "address_entity": ("aml_intelligence_claims", "intelligence", "rebuild_from_reviewed_claims"),
    "address_entities": ("aml_intelligence_claims", "intelligence", "rebuild_from_reviewed_claims"),
    "exchange_addresses": ("aml_intelligence_claims", "intelligence", "rebuild_from_reviewed_claims"),
    "address_cluster_claims": ("aml_cluster_claims", "clustering", "preserve_pair_or_member_claim"),
    "address_cluster_memberships": ("aml_cluster_memberships", "clustering", "preserve_membership_provenance"),
    "cluster_versions": (None, "clustering", "needs_cluster_version_catalog"),
    "cluster_runs": ("aml_cluster_runs", "clustering", "normalize_run_and_invalidation"),
    "exposure_seeds": ("aml_exposure_seeds", "exposure", "derive_from_approved_claim"),
    "exposure_runs": ("aml_exposure_runs", "exposure", "normalize_run_and_invalidation"),
    "address_exposure_paths": ("aml_exposure_paths", "exposure", "preserve_allocation_and_path_evidence"),
    "address_exposure": (None, "exposure", "aggregate_is_not_path_evidence"),
    "risk_policies": ("aml_risk_policies", "risk", "preserve_policy_version"),
    "risk_policy_rules": ("aml_risk_policy_rules", "risk", "preserve_policy_version"),
    "wallet_risk_signals": ("aml_wallet_risk_signals", "risk", "preserve_signal_provenance"),
    "wallet_risk_assessments": ("aml_wallet_risk_assessments", "risk", "score_is_not_probability"),
    "protocol_contract_registry": (None, "registry", "needs_reviewed_protocol_registry"),
    "analysis_subjects": (None, "snapshots", "needs_snapshot_and_subject_contract"),
    "wallet_analysis_evidence": (None, "snapshots", "needs_snapshot_and_subject_contract"),
    "wallet_analysis_snapshots": (None, "snapshots", "needs_snapshot_and_subject_contract"),
    "wallet_asset_balance_deltas_v3": (None, "holdings", "rebuild_from_canonical_evidence"),
    "wallet_asset_balances": (None, "holdings", "rebuild_from_canonical_evidence"),
    "exchange_flows_canonical": (None, "holdings", "rebuild_from_canonical_and_reviewed_claims"),
    "service_boundary_addresses": (None, "intelligence", "rebuild_from_reviewed_claims"),
    "wallet_ml_feature_snapshots": (None, "ml", "retain_existing_ml_history_no_bitcoin_ml"),
    "wallet_ml_model_registry": (None, "ml", "retain_existing_ml_history_no_bitcoin_ml"),
    "wallet_ml_model_deployments": (None, "ml", "retain_existing_ml_history_no_bitcoin_ml"),
    "wallet_ml_predictions": (None, "ml", "retain_existing_ml_history_no_bitcoin_ml"),
    "bitcoin_sync_state": ("aml_sync_state", "core", "implemented_bitcoin_backfill"),
    "bitcoin_block_commits": ("aml_ingested_blocks", "core", "implemented_bitcoin_backfill"),
    "bitcoin_transactions": ("aml_transactions", "core", "implemented_bitcoin_backfill"),
    "bitcoin_inputs": ("aml_bitcoin_inputs", "utxo", "implemented_bitcoin_backfill"),
    "bitcoin_outputs": ("aml_bitcoin_outputs", "utxo", "implemented_bitcoin_backfill"),
    "bitcoin_utxos": (None, "utxo", "implemented_canonical_utxo_query"),
}
VIEW_BASES = {
    "bitcoin_blocks_canonical": "bitcoin_block_commits", "bitcoin_sync_current": "bitcoin_sync_state",
    "address_exposure_best_paths": "address_exposure_paths", "intelligence_active": "intelligence_claims",
    "mv_wallet_asset_delta_transfer_from_v3": "wallet_asset_balance_deltas_v3",
    "mv_wallet_asset_delta_transfer_to_v3": "wallet_asset_balance_deltas_v3",
}
ALIASES = {
    "chain": "network_id", "block_height": "block_number", "block_time": "block_timestamp_unix_ms",
    "timestamp": "block_timestamp_unix_ms", "block_timestamp": "block_timestamp_unix_ms",
    "txid": "tx_hash", "tx_index": "transaction_index", "prev_txid": "prev_tx_hash",
    "prev_vout": "prev_output_index", "fee_sats": "fee_raw", "fee_paid": "fee_raw", "fee": "fee_raw",
    "value": "amount_raw", "amount": "amount_raw", "token_address": "asset_id",
    "token_standard": "asset_standard", "attempt_count": "attempts", "stage": "operation",
    "error_class": "error_code", "error_summary": "error_message", "reviewer": "reviewed_by",
    "label_id": "claim_id", "source_label_id": "claim_id", "source_claim_id": "claim_id",
    "source_record_id": "source_reference", "claim_method": "heuristic", "claim_type": "heuristic",
    "subject_address": "subject_id", "last_synced_block": "last_block_number",
    "last_synced_block_hash": "last_block_hash", "last_finalized_block": "last_block_number",
    "last_finalized_block_hash": "last_block_hash", "start_height": "start_block",
    "tip_height": "last_block_number", "tip_hash": "last_block_hash", "published_at": "published_at_unix_ms",
    "completed_blocks": "blocks", "transaction_count": "transactions", "propagation_run_id": "run_id",
    "path_addresses": "path_subjects", "seed_category": "category", "contract_address": "address",
    "next_attempt_at_unix_ms": "next_attempt_unix_ms", "last_error_class": "last_error",
    "initiator_address": "from_address", "target_address": "to_address",
}
JSON_COLUMNS = {"aml_transactions": "raw_evidence_json", "aml_transaction_features": "features_json",
                "aml_semantic_aml_events": "evidence_json", "aml_cluster_claims": "evidence_json",
                "aml_wallet_risk_signals": "evidence_json", "aml_ingestion_benchmarks": "settings_json"}
REBUILD_STRATEGIES = {"rebuild_from_reviewed_claims", "derive_from_approved_claim"}

# Call-chain anchors cover generic/dynamic dispatch that a table-name grep misses.
# Each anchor is checked against current source; deployment/liveness is not inferred.
OWNERS = {
    "tron": [
        ("core,semantic,metadata", "src/services/tron/batcher/impls.rs", 'const TABLE:', "writer bindings"),
        ("core,semantic,metadata", "src/services/tron/batcher/generic.rs", 'self.clickhouse.insert::<T>(T::TABLE)', "generic writer dispatch"),
        ("core,operations", "src/services/tron/ingestion_state.rs", 'ingested_blocks', "commit/failure writer"),
        ("core", "src/db/sync_state.rs", 'sync_state', "checkpoint owner"),
        ("metadata", "src/services/tron/tron_metadata_worker.rs", 'pub async fn run_token_metadata_worker', "metadata worker"),
        ("intelligence,clustering,exposure", "src/services/tron/entity_intelligence.rs", 'pub async fn review_intelligence_subject', "review and materialized projections"),
        ("clustering", "src/services/tron/address_clustering.rs", 'pub async fn discover_address_cluster_claims', "candidate discovery"),
        ("exposure", "src/tasks/exposure_task.rs", 'insert::<AddressExposureRow>', "persist aggregate exposure"),
        ("clustering,exposure", "src/tasks/analytics.rs", 'async fn cycle(', "periodic analytics caller; file cursor/status"),
        ("snapshots", "src/services/tron/analytical_node.rs", 'insert::<WalletAnalysisSnapshotInsertRow>(SNAPSHOT_TABLE)', "snapshot constant dispatch"),
        ("ml", "src/services/tron/wallet_ai_risk.rs", 'insert::<WalletMlPredictionInsertRow>', "existing inference writer"),
        ("holdings", "src/services/tron/wallet_holdings.rs", 'wallet_asset_balances', "holdings reader"),
        ("core,intelligence,clustering,exposure", "src/services/tron/neo4j/flow_graph.rs", 'address_relationships_canonical', "graph reader"),
    ],
    "ethereum": [
        ("core,semantic,metadata,operations", "src/storage/writer.rs", 'pub async fn persist_block', "generic insert_rows owner"),
        ("metadata", "src/ethereum/token_metadata.rs", 'store.persist_token_metadata', "metadata worker calls store"),
        ("intelligence", "src/intelligence.rs", 'pub async fn import_entity_labels_csv', "CSV import/review/projection"),
        ("clustering", "src/clustering.rs", 'pub async fn discover_address_clusters', "persist claims/memberships/run"),
        ("exposure", "src/exposure.rs", 'pub async fn propagate_exposure', "persist exposure paths/run"),
        ("clustering,exposure", "src/bin/ethereum_analytics_worker.rs", 'discover_address_clusters(&config)', "periodic worker caller"),
        ("risk", "src/risk.rs", 'insert_rows(&self.client, "wallet_risk_signals"', "request-time risk persistence"),
        ("core,semantic,intelligence,clustering,exposure", "src/investigation.rs", 'transactions_canonical', "investigation reader"),
        ("holdings,metadata", "src/ethereum/holdings.rs", 'token_metadata_canonical', "holdings reader"),
    ],
    "bsc": [
        ("core,semantic,operations,metadata,registry", "src/ingestion/store.rs", 'insert::<RelationshipRow>', "ingestion/registry/benchmark writer"),
        ("core,semantic,operations", "src/ingestion/service.rs", 'transactions_canonical', "resume/repair canonical reader"),
        ("metadata", "src/metadata.rs", 'pub async fn worker_once', "metadata writer and RPC holdings"),
        ("metadata", "src/bin/bsc_token_metadata_worker.rs", 'worker_once(&db,&rpc)', "worker caller"),
        ("intelligence,clustering,exposure", "src/intelligence.rs", 'pub async fn exposure(', "claims stored; candidates/exposure computed at request time"),
        ("intelligence,metadata", "src/db/warehouse.rs", 'INSERT INTO {table} FORMAT JSONEachRow', "allowlisted dynamic insert"),
        ("core,semantic,metadata", "src/investigation/wallet.rs", 'transactions_canonical', "investigation reader"),
    ],
    "bitcoin": [
        ("core,utxo,semantic", "src/db/warehouse.rs", 'pub fn map_evidence(', "five fact-table mappings; typed UTXO legs and versioned single-transaction semantic events"),
        ("core,utxo,semantic", "src/storage/writer.rs", 'publication::facts(', "generic facts writer; semantic counts and detector manifest share block publication"),
        ("semantic", "src/semantics.rs", 'pub fn block_facts(', "single-transaction patterns published with block; no ownership or risk inference"),
        ("semantic", "src/semantics.rs", 'pub async fn read(', "bounded canonical pattern reader, historical fallback and exact-outpoint peel-chain context; captured in persisted risk inputs"),
        ("core,utxo", "src/db/warehouse.rs", 'pub async fn backfill(', "legacy read-only migration owner"),
        ("core,utxo,holdings", "src/db/warehouse.rs", 'pub fn utxos()', "canonical readers preserve BIP30 occurrence"),
        ("metadata", "src/assets.rs", 'pub async fn initialize(', "idempotent native BTC protocol-definition writer in the existing asset table; rejects conflicting metadata"),
        ("metadata", "src/assets.rs", 'pub async fn read(', "visible native metadata and stable definition snapshot; no human intelligence approval or supply estimate"),
        ("holdings,utxo", "src/holdings.rs", 'pub async fn read(', "exact canonical latest-outpoint states before address filter, coinbase maturity and bound snapshot; aggregate totals independent of output list limit"),
        ("clustering", "src/clustering/store.rs", 'pub async fn compute(', "bounded canonical common-input/change leads; existing cluster run/claim/membership/version tables and publication seal; no ownership or risk inference"),
        ("clustering", "src/clustering/store.rs", 'pub async fn review(', "explicit optimistic lead-only review; immutable versions, separate review lock and source/review freshness checks"),
        ("clustering", "src/clustering/store.rs", 'pub async fn inspect(', "verify the complete stored output set, publication, detector and latest reviewed version; preserve historical candidates"),
        ("core", "src/db/queries.rs", 'publication::state(', "checkpoint/current-state reader"),
        ("core,utxo,activity", "src/activity.rs", 'pub async fn read(', "scoped canonical wallet aggregates, UTC activity, exact occurrence fees and contextual peers; no feature-table writer"),
        ("operations", "src/bitcoin/failures.rs", 'aml_ingestion_failures', "failure writer and current-state reader"),
        ("operations,core,utxo", "src/analytics/engine.rs", 'async fn discover(', "continuous bounded canonical address sweeps with a durable local operational cursor; no new warehouse table"),
        ("activity,semantic,clustering,exposure,risk", "src/analytics/engine.rs", 'async fn analyze(', "worker reuses the canonical investigation pipeline; fingerprint/semantic inputs in existing risk assessment, existing sealed cluster/exposure outputs; never approves claims"),
        ("intelligence", "src/intelligence/mod.rs", 'pub async fn review(', "separate reviewed attribution publication and durable review evidence"),
        ("intelligence", "src/intelligence/mod.rs", 'pub async fn read(', "address-scoped active source/review/validity reader and snapshot"),
        ("exposure", "src/exposure/mod.rs", 'pub async fn compute(', "bounded canonical UTXO tracing, captured seed snapshots, derived rows and publication seal"),
        ("exposure", "src/exposure/mod.rs", 'pub async fn inspect(', "run reader verifies chain, publication, row IDs/counts and attribution dependencies"),
        ("risk", "src/risk.rs", 'pub async fn record(', "shared compiled policy; persist policy/rules, assessment/signals and sealed publication"),
        ("risk", "src/risk.rs", 'pub async fn inspect(', "read structured assessment and verify rows, policy and attribution/exposure dependencies"),
    ],
}


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def normalized_hash(path):
    return hashlib.sha256(path.read_text(encoding="utf-8").replace("\r\n", "\n").encode()).hexdigest()


def relative(path):
    return path.relative_to(ROOT).as_posix()


def object_mapping(name):
    base = VIEW_BASES.get(name, name)
    if base not in DESTINATIONS:
        base = re.sub(r"_(canonical|current|active|latest)$", "", base)
    if base not in DESTINATIONS:
        raise ValueError(f"Unreviewed legacy object: {name}")
    target, family, strategy = DESTINATIONS[base]
    return {"base": base, "target_table": target, "family": family, "strategy": strategy}


def unit(chain, table, column, data_type):
    if column.endswith("_unix_ms") or column in ("elapsed_ms", "investigation_latency_ms"):
        return "duration_ms" if column in ("elapsed_ms", "investigation_latency_ms") else "unix_ms"
    if column in ("block_time",):
        return "unix_seconds"
    if column in ("timestamp", "block_timestamp"):
        return "unix_ms" if chain == "tron" else "requires_unit_decision"
    if column == "published_at" and chain == "bitcoin":
        return "unix_ms"
    if data_type.startswith("DateTime"):
        return "clickhouse_datetime; preserve server timezone and original precision"
    if column in ("value_sats", "fee_sats", "output_sats"):
        return "satoshi_integer"
    if column in ("amount", "amount_raw", "amount_in", "amount_out", "value", "fee", "fee_raw", "fee_paid", "total_supply", "balance_raw"):
        return "asset_atomic_integer; decimals from verified asset metadata (native BTC=8, ETH/BNB=18, TRX=6)"
    if column in ("confidence", "classification_confidence", "evidence_confidence", "signal_strength", "amount_share", "time_weight", "best_path_amount_share", "best_path_time_weight", "exposure_score", "latest_confidence", "risk_probability", "latest_risk_probability", "model_quality_score", "hop_decay"):
        return "ratio_0_1; validate legacy producer, no implicit percent conversion"
    if column in ("risk_percent", "risk_score", "max_contribution", "medium_threshold", "high_threshold", "contribution", "seed_risk_level"):
        return "score_0_100; not a calibrated probability"
    if column == "risk_level":
        return "score_0_100" if data_type.startswith("UInt") else "categorical_risk_level"
    if column in ("revision", "state_revision", "block_state_revision", "current_revision", "source_revision", "registry_revision", "cluster_version", "version"):
        return "revision_order; different legacy domains, not interchangeable"
    if "bytes" in column or column in ("bytes_written", "compressed_bytes"):
        return "bytes"
    if column.endswith("_per_second"):
        return "rate_per_second"
    if column in ("block_number", "last_block_number", "block_height", "as_of_block", "start_block", "end_block", "discovered_block") or column.endswith(("_block", "_height")):
        return "block_height"
    if column in ("decimals",):
        return "base10_exponent; unknown is nullable, never defaulted to zero"
    if column.startswith("is_") or column in ("enabled", "canonical", "coinbase", "initialized", "retryable", "same_entity", "service_mediated", "status_known", "decimals_known", "balance_incomplete", "metadata_verified", "risk_available", "latest_risk_available", "probability_claimed", "fund_continuity_claimed", "truncated", "receipt_data_complete", "trace_data_complete", "receipt_coverage", "trace_coverage", "prevout_coverage"):
        return "boolean_0_1; NULL coverage preserves unknown/not_applicable where nullable"
    if column in ("status", "transaction_type") and data_type.startswith("UInt"):
        return "chain_execution_or_envelope_enum; preserve unknown separately, not a universal boolean"
    if column in ("gas_used", "gas_limit", "blob_gas_used"):
        return "blob_gas_units" if column == "blob_gas_used" else "EVM_gas_units"
    if column in ("effective_gas_price", "max_fee_per_gas", "max_priority_fee_per_gas", "blob_gas_price"):
        return "native_atomic_units_per_blob_gas" if column == "blob_gas_price" else "native_atomic_units_per_gas"
    if column in ("energy_usage_total", "net_usage"):
        return "TRON_energy_units" if column == "energy_usage_total" else "TRON_bandwidth_units"
    if column == "balance_decimal":
        return "derived_asset_display_units_float; never use as migration amount evidence"
    if column == "direction" and data_type == "Int8":
        return "signed_flow_direction; -1 debit, +1 credit"
    if column == "id":
        return "legacy_local_singleton_identifier; not network_id"
    if column == "schema_version":
        return "immutable_schema_version_identifier"
    if column == "nonce":
        return "sender_transaction_sequence_number"
    if column.endswith("_index") or column in ("tx_index", "prev_vout"):
        return "original_position_index; preserve native/trace/decoder sentinel semantics"
    if column in ("window_days", "time_half_life_days"):
        return "days"
    if column in ("hop_count", "hop_distance", "max_hops", "graph_depth", "exposure_min_hop_distance"):
        return "graph_hops; UTXO transaction-hop semantics require explicit adapter contract"
    if column == "live_rate_multiple":
        return "dimensionless_ingestion_to_observed_live_rate_ratio"
    if column.endswith("_count") or column in ("active_parts", "attempt_count", "attempts", "completed_blocks", "control_claims", "entity_memberships", "incoming_transfers", "outgoing_transfers", "observed_transfers", "max_paths_per_subject", "participants", "requested_blocks", "rows_before", "rows_after", "unique_assets", "blocks", "transactions", "rows_written"):
        return "integer_count: " + column
    if data_type.startswith(("UInt", "Int", "Float", "Nullable(UInt", "Nullable(Float")):
        return "requires_unit_decision"
    if column.endswith("_json"):
        return "versioned_JSON_text; preserve names, numeric precision, original types and evidence"
    if column == "trace_address":
        return "ordered_EVM_trace_child_indexes"
    return "identifier_enum_text_or_structured_evidence; preserve exact value and namespace"


def column_mapping(chain, obj, column, mapping, targets):
    name, source_type = column["name"], column["type"]
    target_name = mapping["target_table"]
    result = {"source": name, "type": source_type,
              "unit": unit(chain, obj["name"], name, source_type),
              "default_kind": column.get("default_kind", ""), "default_expression": column.get("default_expression", ""),
              "compression_codec": column.get("compression_codec", ""), "comment": column.get("comment", "")}
    if not target_name:
        return dict(result, disposition="retain_legacy_until_replacement", target=None, rule=mapping["strategy"])
    fields = {c["name"]: c["data_type"] for c in targets[target_name]["columns"]}
    dest = name if name in fields else ALIASES.get(name)
    base = mapping["base"]
    # Same spelling does not establish the same semantics.
    special = {
        ("bitcoin_transactions", "output_sats"): ("raw_evidence_json", "preserve_output_total; amount_raw remains NULL for multi-party UTXO"),
        ("bitcoin_sync_state", "id"): ("network_id", "replace local singleton id with validated Bitcoin network identity"),
        ("address_cluster_memberships", "address"): ("subject_id", "typed address identity"),
        ("address_cluster_claims", "address"): ("subject_id", "typed address identity; preserve evidence and claim method"),
        ("wallet_risk_signals", "address"): ("subject_id", "typed address identity"),
        ("wallet_risk_assessments", "address"): ("subject_id", "typed address identity"),
        ("address_exposure_paths", "subject_address"): ("subject_id", "typed address identity"),
        ("token_metadata", "is_verified"): ("review_status", "verification is not analyst approval; preserve source and do not auto-approve"),
    }
    if (base, name) in special:
        dest, rule = special[(base, name)]
    elif name == "commit_id":
        rule = "allocate new commit identity with verified block hash and shared revision; retain legacy commit as migration evidence, do not copy"
    elif name == "source" and "source_id" in fields:
        dest, rule = "source_id", "resolve original source to registered network-scoped source id; preserve original text and provenance"
    elif name in ("initiator_address", "target_address"):
        rule = "preserve original transaction endpoint; contract call target does not prove a value-transfer relationship"
    elif chain == "bitcoin" and name == "address":
        rule = "empty legacy address becomes NULL; script remains a typed script subject, not an invented address"
    elif name in ("revision", "state_revision", "block_state_revision", "current_revision", "inserted_at", "updated_at"):
        return dict(result, disposition="retain_legacy_until_replacement", target=None,
                    rule="preserve original version/time; allocate independent monotonic publication/record revision; never copy as shared revision")
    elif base == "sync_state" and name in ("next_block", "updated_at_unix_ms"):
        return dict(result, disposition="retain_legacy_until_replacement", target=None,
                    rule="next-to-fetch and update time are not a verified committed cursor or publication time")
    elif name == "contract_address" and base != "evm_logs":
        dest, rule = None, "contract metadata is not a transfer recipient"
    elif name == "transaction_count" and base in ("ingested_blocks", "bitcoin_block_commits"):
        dest, rule = "transaction_count", "exact declared block transaction count"
    elif name == "subject_id" or dest == "subject_id":
        rule = "normalize typed subject (address/transaction/script); preserve source identity"
    elif name == "chain":
        rule = "map legacy chain spelling to validated deployment network_id; reject mixed networks"
    elif name in ("token_address", "asset_in", "asset_out"):
        rule = "normalize network-scoped asset identity, native sentinel and token standard; never copy address as a universal asset id"
    elif name == "block_time":
        rule = "checked multiply by 1000 (u64 overflow rejected)"
    elif name in ("status", "ingestion_status", "review_status", "decision", "trust_tier", "heuristic", "claim_type", "claim_method", "is_active"):
        rule = "explicit enum/state transition map required; preserve history, unknown and revoked states"
    elif name == "cluster_version":
        rule = "network-scoped cluster version identity; retain original numeric version and provenance"
    elif name in ("receipt_data_complete", "trace_data_complete"):
        dest = {"receipt_data_complete": "receipt_coverage", "trace_data_complete": "trace_coverage"}[name]
        rule = "known coverage only; nullable not_applicable must not become false or complete"
    else:
        rule = "validate identity, range, nullability and producer semantics before conversion"
    if dest in fields:
        # Claim-derived address/entity/exchange projections are not original claims.
        disposition = "reconstruct_from_provenance" if mapping["strategy"] in REBUILD_STRATEGIES else "candidate_field_mapping"
        return dict(result, disposition=disposition, target=f"{target_name}.{dest}", target_type=fields[dest], rule=rule)
    payload = JSON_COLUMNS.get(target_name)
    if payload:
        return dict(result, disposition="preserve_in_versioned_json", target=f"{target_name}.{payload}",
                    target_type=fields[payload], rule="planned lossless legacy namespace with original field/type/value; preserve existing JSON; not implemented by this audit")
    return dict(result, disposition="retain_legacy_until_replacement", target=None,
                rule="no lossless v1 field; DB-03/DB-05 extension or equivalent versioned evidence required before cutover")


def references(folder, table):
    """Classify literal call sites, keeping unproven references separate.

    Generic dispatch is verified separately by OWNERS; allowlists and struct names
    alone cannot establish a writer. Test-module tails are excluded; a cfg(test)
    import/field earlier in an otherwise active module must not hide its writers.
    """
    result = []
    word = re.compile(r"(?<![A-Za-z0-9_])" + re.escape(table) + r"(?![A-Za-z0-9_])")
    for path in sorted((ROOT / folder / "src").rglob("*.rs")):
        content = re.split(r"#\[cfg\(test\)\]\s*mod\s+tests\b", path.read_text(encoding="utf-8"), maxsplit=1)[0]
        aliases = re.findall(r'const\s+(\w+)[^=;]*=\s*"' + re.escape(table) + r'"', content)
        for match in word.finditer(content):
            start = content.rfind("\n", 0, match.start()) + 1
            end = content.find("\n", match.end())
            line = content.count("\n", 0, match.start()) + 1
            prefix = content[max(0, match.start() - 160):match.start()]
            if "/db/" in path.as_posix() and table != "schema_migrations":
                role = "schema_or_adapter_reference"
            else:
                role = "unproven_reference"
            if re.search(r'(?:INSERT\s+INTO|FROM|JOIN)\s+(?:\{[^}]+\}\.|[\w]+\.)?$', prefix, re.I):
                role = "literal_writer" if re.search(r'INSERT\s+INTO\s+(?:\{[^}]+\}\.|[\w]+\.)?$', prefix, re.I) else "literal_reader"
            elif re.search(r'\.insert(?:::<[^;\n]*>)?\(\s*"$', prefix) or re.search(r'insert_(?:rows|one)\(\s*&?[\w.]+,\s*"$', prefix):
                role = "insert_call_writer"
            elif re.search(r'const TABLE[^;\n]*=\s*"$', prefix):
                role = "writer_binding_requires_generic_dispatch"
            result.append({"path": relative(path), "line": line, "role": role,
                           "text": content[start:end if end != -1 else len(content)].strip()})
        for alias in aliases:
            for match in re.finditer(r'\.insert(?:::<[^;\n]*>)?\(\s*' + re.escape(alias) + r'\)', content):
                result.append({"path": relative(path), "line": content.count("\n", 0, match.start()) + 1,
                               "role": "resolved_constant_writer", "text": match.group(), "binding": alias})
    # The ML trainer exports SQL, it does not directly write to ClickHouse.
    if table in ("wallet_ml_model_registry", "wallet_ml_model_deployments"):
        path = ROOT / "dockerizd_tron/ml/tron_wallet_risk/train.py"
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if "INSERT INTO tron_db." + table in line:
                result.append({"path": relative(path), "line": number, "role": "sql_export_producer", "text": line.strip()})
    return sorted(result, key=lambda r: (r["path"], r["line"], r["role"]))


def owner_evidence(chain):
    result = []
    for families, file, anchor, role in OWNERS[chain]:
        path = ROOT / CHAINS[chain][0] / file
        content = path.read_text(encoding="utf-8")
        if anchor not in content:
            raise ValueError(f"Stale owner evidence: {relative(path)}: {anchor}")
        result.append({"families": families.split(","), "path": relative(path),
                       "line": content[:content.index(anchor)].count("\n") + 1, "anchor": anchor, "role": role})
    return result


def sql_export_conflicts(snapshot):
    """Audit the existing trainer's SQL output without executing/training it."""
    path = ROOT / "dockerizd_tron/ml/tron_wallet_risk/train.py"
    content = path.read_text(encoding="utf-8")
    tables = {o["name"]: {c["name"] for c in o["columns"]} for o in snapshot["chains"]["tron"]["objects"]}
    result = []
    for match in re.finditer(r"INSERT INTO tron_db\.(\w+)\s*\((.*?)\)\s*VALUES", content, re.S):
        table, fields = match.group(1), [c.strip() for c in match.group(2).split(",")]
        missing = [field for field in fields if field not in tables.get(table, set())]
        if missing:
            result.append({"path": relative(path), "line": content[:match.start()].count("\n") + 1,
                "table": table, "status": "table_absent_from_current_DDL" if table not in tables else "export_columns_absent_from_current_DDL",
                "missing_columns": missing, "action": "retain existing history; do not execute exporter SQL unchanged; resolve compatibility in DB-05/DB-08 without adding Bitcoin ML"})
    return result


def ttl(obj):
    query = obj["create_table_query"]
    if " TTL " not in query:
        return "no automatic TTL in checked DDL (not a backup or retention policy)"
    return query.split(" TTL ", 1)[1].split(" SETTINGS ", 1)[0]


def indexes(obj):
    return re.findall(r"INDEX\s+(.+?GRANULARITY \d+)", obj["create_table_query"])


def dependencies(obj, objects):
    return sorted(name for name in (o["name"] for o in objects) if name != obj["name"]
                  and re.search(r"(?<![A-Za-z0-9_])" + re.escape(name) + r"(?![A-Za-z0-9_])", obj["as_select"]))


def materialized_target(obj):
    match = re.search(r"\bTO \{\{database\}\}\.(\w+)", obj["create_table_query"])
    return match.group(1) if match else None


def access_summary(obj, refs, data):
    seeds = []
    for item in data["ddl_manifest"]:
        path = ROOT / item["path"]
        content = path.read_text(encoding="utf-8")
        pattern = r"INSERT\s+INTO\s+(?:\{\{database\}\}|tron_db)\." + re.escape(obj["name"]) + r"\b"
        for match in re.finditer(pattern, content, re.I):
            seeds.append({"path": relative(path), "line": content[:match.start()].count("\n") + 1, "role": "DDL_seed_insert"})
    return {
        "literal_or_helper_writers": [r for r in refs if r["role"] in ("literal_writer", "insert_call_writer", "resolved_constant_writer")],
        "generic_writer_bindings": [r for r in refs if r["role"] == "writer_binding_requires_generic_dispatch"],
        "literal_readers": [r for r in refs if r["role"] == "literal_reader"],
        "seed_writers": seeds,
        "materialized_writers": [o["name"] for o in data["objects"] if materialized_target(o) == obj["name"]],
        "dependent_views": [o["name"] for o in data["objects"] if obj["name"] in dependencies(o, data["objects"])],
        "sql_export_producers": [r for r in refs if r["role"] == "sql_export_producer"],
        "absence_semantics": "empty lists mean no verified call site in this audit; family ownership alone is not a writer for this table",
    }


def required_inputs(target, columns):
    """Make absent target fields visible instead of suggesting a row can be copied."""
    mapped = {c["target"].rsplit(".", 1)[1] for c in columns if c["target"]}
    recipes = {
        "network_id": "validated adapter identity; reject mixed/wrong network, never infer from address shape",
        "revision": "allocate monotonic shared revision under single writer; legacy versions are independent",
        "commit_id": "allocate publication identity tied to verified block hash/revision",
        "block_hash": "fetch verified canonical block header; a height-only join is insufficient",
        "block_number": "verified original block occurrence; never an inferred analysis height",
        "block_timestamp_unix_ms": "verified block header time with checked unit conversion",
        "transaction_index": "original order in verified block; never constant zero for all transactions",
        "created_at_unix_ms": "preserve known source creation time; separately record migration time/provenance",
        "receipt_coverage": "known receipt completeness or NULL for not_applicable/unknown",
        "trace_coverage": "known trace completeness or NULL for not_applicable/unknown",
        "prevout_coverage": "all prevouts resolved or explicit incomplete; NULL outside UTXO capability",
        "source_revision": "pin published input fence plus analysis invalidation/version contract before computing",
        "evidence_refs": "stable references to actual network/tx/outpoint/source/review evidence; no fabricated refs",
        "subject_id": "typed network-scoped subject derived from original evidence; pair claims need both endpoints",
        "from_subject_id": "observed sender/script/transaction node; no Cartesian UTXO attribution",
        "to_subject_id": "observed recipient/script/transaction node; no Cartesian UTXO attribution",
        "from_subject_type": "explicit address/script/transaction capability from original evidence",
        "to_subject_type": "explicit address/script/transaction capability from original evidence",
        "decimals": "verified asset decimals; unknown remains nullable where permitted, no zero assumption",
        "review_status": "preserve pending/approved/rejected/revoked chronology; verification does not imply approval",
        "reviewed_by": "original accountable reviewer; no invented analyst or automatic approval",
        "policy_version": "pin actual versioned policy; do not substitute detector version",
        "amount_raw": "exact asset atomic units; Bitcoin transaction total stays NULL, individual UTXO legs carry satoshis",
        "probability_claimed": "false for score-only assessments; probability requires separate calibrated evidence",
        "fund_continuity_claimed": "false for heuristic/UTXO allocation without proof; preserve allocation method and limits",
    }
    return [{"column": c["name"], "type": c["data_type"], "recipe": recipes.get(c["name"],
             "requires explicit source evidence/derivation and validation before cutover; no implicit default")}
            for c in target["columns"] if c["name"] not in mapped]


def refresh(client):
    snapshot = inventory(client)
    snapshot["scope"] = "fresh disposable ClickHouse DDL fixtures plus code-defined migration ledgers and shared v1; no production rows"
    snapshot["clickhouse_version"] = client.execute("SELECT version() AS version", rows=True)[0]["version"]
    snapshot["source_manifest"] = []
    for chain, data in snapshot["chains"].items():
        database = data["database"]
        if chain in ("ethereum", "bsc"):
            path = ROOT / CHAINS[chain][0] / "src/db/migrations.rs"
            source = path.read_text(encoding="utf-8")
            match = re.search(r'CREATE TABLE IF NOT EXISTS \{database\}\.schema_migrations\s*\([\s\S]*?ORDER BY migration_id', source)
            if not match:
                raise ValueError("Runtime migration ledger DDL was not found")
            client.execute(match.group().replace("{database}", database), database=database)
            snapshot["source_manifest"].append({"path": relative(path), "normalized_sha256": normalized_hash(path)})
        for sql in statements(SQL.read_text(encoding="utf-8").replace("{{database}}", database)):
            client.execute(sql, database=database)
        objects = client.execute("SELECT name,engine,engine_full,partition_key,sorting_key,primary_key,create_table_query,as_select "
                                 f"FROM system.tables WHERE database='{database}' ORDER BY name", rows=True)
        columns = client.execute("SELECT table,name,type,position,default_kind,default_expression,compression_codec,comment "
                                 f"FROM system.columns WHERE database='{database}' ORDER BY table,position", rows=True)
        for obj in objects:
            obj["columns"] = [col for col in columns if col["table"] == obj["name"]]
            # Normalize only this known disposable database identifier.
            for field in ("create_table_query", "as_select", "engine_full"):
                obj[field] = obj[field].replace(database + ".", "{{database}}.")
        data["objects"] = [o for o in objects if not o["name"].startswith("aml_")]
        data["shared_objects"] = [o for o in objects if o["name"].startswith("aml_")]
    for path in (CONTRACT, SQL):
        snapshot["source_manifest"].append({"path": relative(path), "normalized_sha256": normalized_hash(path)})
    return snapshot


def validate_snapshot(snapshot):
    if set(snapshot["chains"]) != set(NETWORKS):
        raise ValueError("All four isolated network fixtures are required")
    for item in snapshot["source_manifest"] + [item for data in snapshot["chains"].values() for item in data["ddl_manifest"]]:
        if normalized_hash(ROOT / item["path"]) != item["normalized_sha256"]:
            raise ValueError(f"DDL/source changed since materialization: {item['path']}; refresh disposable snapshot")
    for chain, (folder, explicit) in CHAINS.items():
        directory = ROOT / folder
        paths = ([directory / p for p in explicit] if explicit else
                 sorted((directory / ("sql/migrations" if chain == "bsc" else "sql")).glob("*.sql")))
        if [relative(p) for p in paths] != [p["path"] for p in snapshot["chains"][chain]["ddl_manifest"]]:
            raise ValueError(f"DDL file coverage changed: {chain}; refresh disposable snapshot")
        names = {"schema_migrations"} if chain in ("ethereum", "bsc") else set()
        for path in paths:
            for statement in statements(path.read_text(encoding="utf-8")):
                match = re.match(r"(CREATE|DROP)\s+(?:OR REPLACE\s+)?(?:MATERIALIZED VIEW|TABLE|VIEW)\s+"
                                 r"(?:IF (?:NOT )?EXISTS\s+)?(?:(?:\{\{database\}\}|tron_db)\.)?(\w+)\b", statement, re.I)
                if match:
                    if match.group(1).upper() == "CREATE":
                        names.add(match.group(2))
                    else:
                        names.discard(match.group(2))
        actual_names = [obj["name"] for obj in snapshot["chains"][chain]["objects"]]
        if set(actual_names) != names or len(set(actual_names)) != len(actual_names):
            raise ValueError(f"Legacy object coverage differs from checked DDL: {chain}")
    expected = snapshot["chains"]["bitcoin"]["shared_objects"]
    for chain, data in snapshot["chains"].items():
        if data["database"] != "aml_inventory_" + chain or data["shared_objects"] != expected:
            raise ValueError(f"Shared fixture shape or database identity differs: {chain}")
    contract = read_json(CONTRACT)
    objects = {o["name"]: o for o in expected}
    if set(objects) != {o["name"] for o in contract["tables"] + contract["views"]}:
        raise ValueError("Shared fixture object coverage differs from contract")
    for table in contract["tables"]:
        actual = [(c["name"], c["type"]) for c in objects[table["name"]]["columns"]]
        if actual != [(c["name"], c["data_type"]) for c in table["columns"]]:
            raise ValueError(f"Shared fixture columns differ: {table['name']}")


def build(snapshot):
    validate_snapshot(snapshot)
    contract = read_json(CONTRACT)
    targets = {t["name"]: t for t in contract["tables"]}
    report = {"scope": snapshot["scope"], "mapping_status": "reviewed static migration plan; field mappings are not an implemented backfill",
              "network_storage": "separate ClickHouse/database/volume per network VM; shared build-time contract only",
              "schema_version": contract["version"], "schema_sha256": normalized_hash(CONTRACT),
              "clickhouse_version": snapshot["clickhouse_version"], "chains": {}, "shared_objects": []}
    report["source_schema_conflicts"] = sql_export_conflicts(snapshot)
    values_path=ROOT/"shared/warehouse/schema/0002_column_contract.json"
    if values_path.exists():
        values_contract=read_json(values_path)
        report["target_value_contract"]={"version":values_contract["version"],"migration_id":values_contract["migration_id"],
            "path":relative(values_path),"normalized_sha256":normalized_hash(values_path),"rule_count":len(values_contract["rules"]),
            "scope":"v2 column rules; physical columns remain the v1 snapshot; see DB03_VALIDATION.md"}
    # DB-01 is an immutable materialized baseline, not a live v3 inventory.
    # Keep its source mappings honest and publish the additive target separately.
    core_path=ROOT/"shared/warehouse/schema/0003_core_evidence.json"
    current_path=ROOT/"docs/warehouse/core-contract-v3.json"
    if core_path.exists():
        core=read_json(core_path)
        current=read_json(current_path)
        if current["version"] != core["version"] or current["migration_id"] != core["migration_id"]:
            raise ValueError("Current contract export does not match core migration")
        for table in current["tables"]:
            expected=targets[table["name"]]["columns"]+core["columns"].get(table["name"],[])
            if table["columns"] != expected:
                raise ValueError("Current contract column export is stale: "+table["name"])
        report["target_core_contract"]={"version":core["version"],"migration_id":core["migration_id"],
            "path":relative(core_path),"normalized_sha256":normalized_hash(core_path),
            "effective_contract":relative(current_path),"effective_contract_sha256":normalized_hash(current_path),
            "tables":len(current["tables"]),"views":len(current["views"]),
            "table_columns":sum(len(t["columns"]) for t in current["tables"]),"added_columns":core["columns"],
            "scope":"v3 target core schema; DB-01 shared_objects remain the historical v1 snapshot; legacy writer/reader cutover remains DB-08"}
    analytics_path=ROOT/"shared/warehouse/schema/0004_analytics_specialized.json"
    if analytics_path.exists():
        delta=read_json(analytics_path)
        effective_path=ROOT/"docs/warehouse/analytics-contract-v4.json"
        effective=read_json(effective_path)
        previous={t["name"]:t for t in current["tables"]}
        added={t["name"]:t for t in delta["tables"]}
        if effective["version"] != delta["version"] or effective["migration_id"] != delta["migration_id"]:
            raise ValueError("Current contract export does not match analytics migration")
        if {t["name"] for t in effective["tables"]} != set(previous)|set(added):
            raise ValueError("Current analytics table coverage is stale")
        expected_views=[]
        for view in current["views"]:
            view=dict(view)
            if view["name"]=="aml_evm_logs_canonical":
                cols=previous["aml_evm_logs"]["columns"]+delta["columns"]["aml_evm_logs"]
                projection=", ".join("f."+c["name"]+" AS "+c["name"] for c in cols)
                view["select"]="SELECT "+projection+" FROM {{database}}.aml_evm_logs AS f FINAL INNER JOIN {{database}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision"
            expected_views.append(view)
        if effective["views"] != expected_views+delta["views"]:
            raise ValueError("Current analytics view export is stale")
        for table in effective["tables"]:
            expected=added[table["name"]]["columns"] if table["name"] in added else previous[table["name"]]["columns"]+delta["columns"].get(table["name"],[])
            if table["columns"] != expected:raise ValueError("Current analytics column export is stale: "+table["name"])
        report["target_analytics_contract"]={"version":delta["version"],"migration_id":delta["migration_id"],
            "path":relative(analytics_path),"normalized_sha256":normalized_hash(analytics_path),
            "effective_contract":relative(effective_path),"effective_contract_sha256":normalized_hash(effective_path),
            "tables":len(effective["tables"]),"views":len(effective["views"]),"table_columns":sum(len(t["columns"]) for t in effective["tables"]),
            "new_tables":list(added),"added_columns":delta["columns"],
            "scope":"v4 target analytics/specialized schema and common readers; worker implementation and legacy cutover remain open; see DB05_ANALYTICS_CONTRACT_FA.md"}
        report["target_analytics_mapping"]=analytics_mapping(snapshot,effective,delta)
    visibility_path=ROOT/"shared/warehouse/schema/0005_visibility.json"
    if visibility_path.exists():
        delta=read_json(visibility_path);previous=read_json(ROOT/"docs/warehouse/analytics-contract-v4.json")
        effective_path=ROOT/"docs/warehouse/current-contract.json";effective=read_json(effective_path)
        if effective["version"]!=5 or effective["migration_id"]!=delta["migration_id"]:
            raise ValueError("Current contract export does not match visibility migration")
        expected={t["name"]:t["columns"]+delta["columns"].get(t["name"],[]) for t in previous["tables"]+delta["tables"]}
        if {t["name"]:t["columns"] for t in effective["tables"]}!=expected:raise ValueError("Current visibility column export is stale")
        views=[dict(v) for v in previous["views"]]
        for view in delta["views"]:
            index=next((i for i,v in enumerate(views) if v["name"]==view["name"]),None)
            if index is None:views.append(view)
            else:views[index]=view
        if effective["views"]!=views:raise ValueError("Current visibility view export is stale")
        sources=[ROOT/"shared/warehouse/src"/name for name in ["visibility.rs","visibility_schema.rs","publication.rs","analytics.rs","core.rs"]]
        report["target_visibility_contract"]={"version":5,"migration_id":delta["migration_id"],"path":relative(visibility_path),"normalized_sha256":normalized_hash(visibility_path),
            "effective_contract":relative(effective_path),"effective_contract_sha256":normalized_hash(effective_path),"tables":len(effective["tables"]),"views":len(effective["views"]),"table_columns":sum(len(t["columns"]) for t in effective["tables"]),
            "source_manifest":[{"path":relative(p),"normalized_sha256":normalized_hash(p)} for p in sources],
            "scope":"DB-07 common canonical/visibility contract and active Bitcoin integration; legacy three-network cutover remains DB-08; finality is not inferred from confirmation depth"}
    relationship_sql=ROOT/"shared/warehouse/src/relationships/query.sql"
    if relationship_sql.exists():
        review_sql=ROOT/"dockerizd_bitcoin/sql/queries/relationships.sql"
        if not review_sql.exists() or normalized_hash(review_sql)!=normalized_hash(relationship_sql):
            raise ValueError("Relationship SQL review artifact is stale")
        sources=[*sorted((ROOT/"shared/warehouse/src/relationships").glob("*.rs")),
            ROOT/"dockerizd_bitcoin/src/api.rs",ROOT/"dockerizd_bitcoin/src/db/warehouse.rs",
            ROOT/"dockerizd_bitcoin/src/domain/model.rs",ROOT/"analytical_node/src/main.rs",
            ROOT/"dockerizd_bitcoin/web/bitcoin.js"]
        report["relationship_interpretation"]={"version":"typed_relationships_v1","introduced_storage_schema_version":4,"storage_schema_version":5,
            "mutates_storage":False,"sql_path":relative(relationship_sql),"sql_sha256":normalized_hash(relationship_sql),
            "review_sql_path":relative(review_sql),"source_manifest":[{"path":relative(p),"normalized_sha256":normalized_hash(p)} for p in sources],
            "bitcoin":"active writer validation and shared canonical graph reader; observed UTXO legs, no input/output allocation",
            "other_networks":"same shared API/model tested; active legacy writer/reader cutover remains DB-08",
             "scope":"DB-06 interpretation remains compatible with the additive v5 visibility contract; historical schema v4 remains immutable; no history conversion"}
    recovery_sources = [ROOT / name for name in (
        "scripts/warehouse_backup.py", "shared/clickhouse/backup_disk.xml",
        "shared/clickhouse/backup.compose.yml", "scripts/provision_vms.py",
        "scripts/provision_vms_real.py", "scripts/tests/warehouse_backup_integration.py")]
    report["migration_recovery"] = {
        "stage": "DB-08A", "storage_schema_version": 5, "schema_changed": False,
        "cutover_allowed": False, "row_encoding": "typed_hex_v1",
        "deployment": "independent backup disk and database on each network VM",
        "source_manifest": [{"path": relative(p), "normalized_sha256": normalized_hash(p)} for p in recovery_sources],
        "acceptance": "docs/warehouse/db08a-acceptance.json",
        "runbook": "docs/warehouse/DB08A_BACKUP_RESTORE_FA.md",
        "remaining": ["DB-08B lossless mappings and reconciliation", "DB-08C active writers/readers and backfill", "DB-08D analytics/worker parity and final cutover"]}
    for chain, data in snapshot["chains"].items():
        owners = owner_evidence(chain)
        objects = []
        for obj in data["objects"]:
            mapping = object_mapping(obj["name"])
            refs = references(CHAINS[chain][0], obj["name"])
            columns = [column_mapping(chain, obj, c, mapping, targets) for c in obj["columns"]]
            objects.append({"name": obj["name"], "kind": obj["engine"], "engine": obj["engine_full"],
                            "partition_key": obj["partition_key"], "sorting_key": obj["sorting_key"],
                            "primary_key": obj["primary_key"], "retention": ttl(obj), "indexes": indexes(obj),
                            "definition": obj["as_select"], "mapping": mapping,
                            "dependencies": dependencies(obj, data["objects"]), "materialized_target": materialized_target(obj),
                            "read_contract": "reconstruct and compare legacy view output; never INSERT a view" if obj["engine"] in ("View", "MaterializedView") else "table migration plan",
                            "runtime_status": "legacy migration source only" if chain == "bitcoin" else "source audited; deployment and actual data not inspected",
                            "columns": columns, "access_evidence": refs,
                            "access_summary": access_summary(obj, refs, data),
                            "required_target_inputs": required_inputs(targets[mapping["target_table"]], columns) if mapping["target_table"] else [],
                            "call_chain_evidence": [e for e in owners if mapping["family"] in e["families"]]})
        report["chains"][chain] = {"network_id": NETWORKS[chain], "fixture_database": data["database"], "objects": objects, "owners": owners}
    purposes = {t["name"]: t["purpose"] for t in contract["tables"] + contract["views"]}
    for obj in snapshot["chains"]["bitcoin"]["shared_objects"]:
        name = obj["name"]
        refs = references("shared/warehouse", name) + references(CHAINS["bitcoin"][0], name)
        report["shared_objects"].append({"name": name, "kind": obj["engine"], "purpose": purposes[name],
            "engine": obj["engine_full"], "partition_key": obj["partition_key"], "sorting_key": obj["sorting_key"],
            "primary_key": obj["primary_key"], "retention": ttl(obj), "indexes": indexes(obj), "definition": obj["as_select"],
            "dependencies": dependencies(obj, snapshot["chains"]["bitcoin"]["shared_objects"]),
            "columns": [dict(c, unit=unit("common", name, c["name"], c["type"])) for c in obj["columns"]],
            "access_evidence": refs,
            "runtime": {chain: shared_runtime(chain, name) for chain in NETWORKS}})
    report["source_manifest"] = [{"path": relative(path), "normalized_sha256": normalized_hash(path)}
        for folder, _ in CHAINS.values() for path in sorted((ROOT / folder / "src").rglob("*.rs"))]
    report["source_manifest"] += [{"path": relative(path), "normalized_sha256": normalized_hash(path)}
        for path in sorted((ROOT / "shared/warehouse/src").rglob("*.rs"))]
    report["source_manifest"].append({"path": "dockerizd_tron/ml/tron_wallet_risk/train.py",
        "normalized_sha256": normalized_hash(ROOT / "dockerizd_tron/ml/tron_wallet_risk/train.py")})
    report["counts"] = {chain: {"objects": len(data["objects"]), "columns": sum(len(o["columns"]) for o in data["objects"]),
        "columns_retained_pending_replacement": sum(c["disposition"] == "retain_legacy_until_replacement" for o in data["objects"] for c in o["columns"])}
        for chain, data in report["chains"].items()}
    report["counts"]["common"] = {"objects": len(report["shared_objects"]), "table_columns": sum(len(t["columns"]) for t in contract["tables"]),
        "view_columns": sum(len(o["columns"]) for o in report["shared_objects"] if o["kind"] == "View")}
    return report


def analytics_mapping(snapshot, contract, delta):
    """Versioned conversion PLAN; retains original names/types alongside candidates.

    This never copies legacy rows or promotes source metadata to analyst approval.
    """
    targets={t["name"]:t for t in contract["tables"]}
    supported=set(delta["columns"])|{t["name"] for t in delta["tables"]}
    overrides={"token_metadata_discoveries":"aml_metadata_discoveries",
        "protocol_contract_registry":"aml_protocol_registry","cluster_versions":"aml_cluster_versions",
        "address_exposure":"aml_address_exposures","address_entity":"aml_entity_addresses",
        "address_entities":"aml_entity_addresses","exchange_addresses":"aml_entity_addresses"}
    aliases={"chain":"network_id","label_id":"claim_id","source":"source_id",
        "source_record_id":"source_record_id","supersedes_label_id":"supersedes_claim_id",
        "reviewer":"reviewed_by","risk_percent":"risk_level","token_address":"asset_id",
        "token_standard":"asset_standard","attempt_count":"attempts","source_claim_id":"claim_id",
        "source_label_id":"claim_id","left_address":"left_subject_id","right_address":"right_subject_id",
        "address":"subject_id","version":"cluster_version","propagation_run_id":"run_id",
        "exposed_address":"subject_id","source_address":"source_subject_id",
        "path_addresses":"path_subjects","subject_address":"subject_id",
        "remote_contract_address":"remote_subject_id","contract_address":"subject_id",
        "best_path_time_weight":"best_path_time_weight","total_supply":"total_supply_raw"}
    result=[]
    for chain, data in snapshot["chains"].items():
        for obj in data["objects"]:
            mapping=object_mapping(obj["name"])
            target=overrides.get(obj["name"],mapping["target_table"])
            if target not in supported:continue
            fields={c["name"] for c in targets[target]["columns"]}
            rows=[]
            for c in obj["columns"]:
                source=c["name"]
                candidate=source if source in fields else aliases.get(source)
                if candidate not in fields:candidate=None
                rows.append({"source":source,"source_type":c["type"],
                    "candidate":target+"."+candidate if candidate else None,
                    "original_evidence_path":f"{target}.raw_evidence_json/legacy_v1/{chain}/{obj['name']}/fields/{source}",
                    "conversion":"preserve original field name/type/value independently; validate candidate type, identity, time, precision and meaning before DB-08 conversion"})
            result.append({"chain":chain,"source_object":obj["name"],"source_kind":obj["engine"],"target_table":target,
                "strategy":"reconstruct view from verified provenance" if obj["engine"] in ("View","MaterializedView") or obj["name"] in ("address_entity","address_entities","exchange_addresses") else "explicit verified DB-08 table conversion",
                "status":"plan only; no backfill or active legacy cutover", "columns":rows})
    return result


def shared_runtime(chain, name):
    if name in ("aml_schema_migrations", "aml_warehouse_identity"):
        return "active shared init/check; separate from legacy ledger"
    if chain != "bitcoin":
        return "installed only; legacy writer/reader cutover remains open"
    if name in {"aml_sync_state", "aml_sync_state_current", "aml_ingested_blocks", "aml_ingested_blocks_canonical",
                "aml_transactions", "aml_transactions_canonical", "aml_address_relationships",
                "aml_address_relationships_canonical", "aml_bitcoin_inputs", "aml_bitcoin_inputs_canonical",
                "aml_bitcoin_outputs", "aml_bitcoin_outputs_canonical", "aml_ingestion_failures", "aml_ingestion_failures_current"}:
        return "active Bitcoin source path; CLI/API/backfill; production data not inspected"
    if name.startswith(("aml_evm_logs", "aml_tron_contracts")):
        return "not_applicable to Bitcoin; common DDL present, no fabricated rows"
    if name.startswith(("aml_intelligence_sources", "aml_intelligence_claims", "aml_intelligence_reviews")):
        return "active Bitcoin source/import/review CLI and investigation attribution; central label risk and exposure seed verification with snapshot invalidation; clustering/background workers remain open"
    if name.startswith(("aml_exposure_runs", "aml_exposure_paths", "aml_address_exposures")):
        return "active bounded on-demand Bitcoin exposure CLI/API, exact-outpoint conservative value bounds and current-run verification; background worker remains open"
    if name.startswith("aml_exposure_seeds"):
        return "captured Bitcoin seed/claim revision snapshots; current eligibility requires the exposure run's attribution check"
    if name.startswith("aml_asset_metadata"):
        return "active Bitcoin compiled native BTC protocol metadata bootstrap/read/verification; current/visible registry, explicit source and stable metadata snapshot; no circulating supply estimate or imported intelligence approval"
    if name.startswith("aml_metadata_jobs"):
        return "not_applicable to immutable native BTC protocol constants; no fabricated discovery or metadata job"
    if name.startswith("aml_semantic_aml_events"):
        return "active Bitcoin versioned single-transaction pattern writer under block fence; bounded canonical reader and historical fallback; peel chains captured in risk inputs/central snapshots; no pattern-only score or ownership; historical fact backfill/background worker remain open"
    if name.startswith(("aml_risk_policies", "aml_risk_policy_rules", "aml_wallet_risk_signals", "aml_wallet_risk_assessments")):
        return "active Bitcoin shared-policy evaluation and local persistence; sealed assessments/signals require the adapter risk reader's policy/intelligence/exposure verification; central consumer recomputes and checks the assessment snapshot"
    return "installed only; Bitcoin capability/writer/reader still open"


def markdown(report, chain):
    common = chain == "common"
    objects = report["shared_objects"] if common else report["chains"][chain]["objects"]
    lines = [f"# DB-01 dictionary: {chain}", "", "Generated by `python scripts/warehouse_data_dictionary.py`. Do not edit this file.",
             "", "Schema/source audit only. Candidate mappings are plans, not executable migrations or feature-completion claims.", ""]
    if common:
        lines += ["This is the immutable v1 materialized baseline. V3 is frozen in [core-contract-v3.json](../core-contract-v3.json) and v4 in [analytics-contract-v4.json](../analytics-contract-v4.json). For the current v5 target, see [the effective contract](../current-contract.json) and [DB-07](../DB07_VISIBILITY_FA.md).", ""]
    for obj in objects:
        lines += [f"## {obj['name']}", "", f"Kind: `{obj['kind']}`; engine: `{obj['engine']}`.", "",
                  f"Partition: `{obj['partition_key'] or 'none'}`; sort/primary: `{obj['sorting_key']}` / `{obj['primary_key']}`.",
                  "", "Retention: " + obj["retention"] + ".", ""]
        if obj["dependencies"]:
            lines += ["SQL dependencies: " + ", ".join(f"`{name}`" for name in obj["dependencies"]) + ".", ""]
        if obj.get("materialized_target"):
            lines += [f"DDL-triggered materialized writer target: `{obj['materialized_target']}` (not a Rust insert call).", ""]
        if common:
            lines += [obj["purpose"], "", "| Column | Type | Unit / semantics |", "|---|---|---|"]
            lines += [f"| `{c['name']}` | `{c['type']}` | {c['unit']} |" for c in obj["columns"]]
            lines += ["", "Runtime: " + "; ".join(f"{n}: {v}" for n, v in obj["runtime"].items()), ""]
        else:
            m = obj["mapping"]
            lines += [f"Family: `{m['family']}`; proposed table: `{m['target_table'] or 'no lossless v1 destination'}`; strategy: `{m['strategy']}`.",
                      "", obj["read_contract"] + ".", ""]
            access = obj["access_summary"]
            lines += [f"Verified direct/helper writers: {len(access['literal_or_helper_writers'])}; generic bindings: {len(access['generic_writer_bindings'])}; direct readers: {len(access['literal_readers'])}.", ""]
            if access["seed_writers"]:
                lines += [f"- DDL seed writer: [{r['path']}:{r['line']}](../../../{r['path']})." for r in access["seed_writers"]] + [""]
            if access["materialized_writers"]:
                lines += ["Materialized writers: " + ", ".join(f"`{name}`" for name in access["materialized_writers"]) + ".", ""]
            if access["dependent_views"]:
                lines += ["Dependent SQL views: " + ", ".join(f"`{name}`" for name in access["dependent_views"]) + ".", ""]
            lines += ["| Source | Type | Unit | Destination / disposition |", "|---|---|---|---|"]
            lines += [f"| `{c['source']}` | `{c['type']}` | {c['unit']} | `{c['target'] or c['disposition']}` — {c['rule']} |" for c in obj["columns"]]
            if obj["required_target_inputs"]:
                lines += ["", "Target inputs not supplied by a source-field mapping:", ""]
                lines += [f"- `{c['column']}` (`{c['type']}`): {c['recipe']}." for c in obj["required_target_inputs"]]
            lines += ["", "Call chains (family ownership; individual direct reads/writes follow below):", ""]
            lines += [f"- [{e['path']}:{e['line']}](../../../{e['path']}) — {e['role']}; `{e['anchor']}`" for e in obj["call_chain_evidence"]]
        lines += ["", "Direct access evidence (unproven references and schema declarations are not runtime writers):", ""]
        refs = [r for r in obj["access_evidence"] if r["role"] not in ("schema_or_adapter_reference", "unproven_reference")]
        lines += [f"- [{r['path']}:{r['line']}](../../../{r['path']}) — {r['role']}; `{r['text'].replace('|', '&#124;')}`" for r in refs] or ["- No direct literal call site found; inspect the checked call chains and JSON reference list."]
        lines += [""]
    return "\n".join(lines) + "\n"


def outputs(report):
    files = {OUTPUT: json.dumps(report, ensure_ascii=False, indent=2) + "\n"}
    for chain in [*NETWORKS, "common"]:
        files[ROOT / f"docs/warehouse/dictionary/{chain}.md"] = markdown(report, chain)
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="offline: fail on stale source/schema/mappings or generated output")
    parser.add_argument("--refresh", action="store_true", help="materialize DDL on a new disposable localhost ClickHouse")
    parser.add_argument("--url")
    parser.add_argument("--disposable", action="store_true")
    args = parser.parse_args()
    if args.refresh:
        if args.check or not args.disposable or not args.url:
            parser.error("--refresh requires --disposable --url and cannot be combined with --check")
        snapshot = refresh(TestClickHouse(args.url, "warehouse_test", "warehouse-test-only"))
        SNAPSHOT.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8", newline="\n")
    report = build(read_json(SNAPSHOT))
    for path, content in outputs(report).items():
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                raise ValueError(f"Stale dictionary: {relative(path)}; regenerate and review")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8", newline="\n")
    print(json.dumps(report["counts"], sort_keys=True))


if __name__ == "__main__":
    main()
