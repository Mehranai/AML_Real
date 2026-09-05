SELECT
    snapshots.address AS address,
    labels.label AS label,
    snapshots.snapshot_id AS snapshot_id,
    'tron_wallet_behavior_features_v2' AS feature_schema_version,
    snapshots.generated_at_unix_ms AS feature_generated_at_unix_ms,
    labels.label_source AS label_source,
    labels.case_id AS label_case_id,
    labels.label_created_at_unix_ms AS label_created_at_unix_ms,
    JSONExtractFloat(snapshots.features_json, 'total_transfers_log') AS total_transfers_log,
    JSONExtractFloat(snapshots.features_json, 'unique_transactions_log') AS unique_transactions_log,
    JSONExtractFloat(snapshots.features_json, 'incoming_transfers_log') AS incoming_transfers_log,
    JSONExtractFloat(snapshots.features_json, 'outgoing_transfers_log') AS outgoing_transfers_log,
    JSONExtractFloat(snapshots.features_json, 'unique_senders_log') AS unique_senders_log,
    JSONExtractFloat(snapshots.features_json, 'unique_receivers_log') AS unique_receivers_log,
    JSONExtractFloat(snapshots.features_json, 'fan_in_score') AS fan_in_score,
    JSONExtractFloat(snapshots.features_json, 'fan_out_score') AS fan_out_score,
    JSONExtractFloat(snapshots.features_json, 'flow_imbalance_score') AS flow_imbalance_score,
    JSONExtractFloat(snapshots.features_json, 'burst_score') AS burst_score,
    JSONExtractFloat(snapshots.features_json, 'swap_ratio') AS swap_ratio,
    JSONExtractFloat(snapshots.features_json, 'bridge_ratio') AS bridge_ratio,
    JSONExtractFloat(snapshots.features_json, 'exchange_interaction_ratio') AS exchange_interaction_ratio,
    JSONExtractFloat(snapshots.features_json, 'contract_call_ratio') AS contract_call_ratio,
    JSONExtractFloat(snapshots.features_json, 'counterparty_concentration') AS counterparty_concentration,
    JSONExtractFloat(snapshots.features_json, 'token_diversity_score') AS token_diversity_score,
    JSONExtractFloat(snapshots.features_json, 'exposure_score') AS exposure_score,
    JSONExtractFloat(snapshots.features_json, 'exposure_source_count_score') AS exposure_source_count_score,
    JSONExtractFloat(snapshots.features_json, 'exposure_path_count_score') AS exposure_path_count_score,
    JSONExtractFloat(snapshots.features_json, 'exposure_min_hop_score') AS exposure_min_hop_score,
    JSONExtractFloat(snapshots.features_json, 'identity_confidence') AS identity_confidence,
    JSONExtractFloat(snapshots.features_json, 'exchange_service_wallet_score') AS exchange_service_wallet_score,
    JSONExtractFloat(snapshots.features_json, 'truncated_sample_score') AS truncated_sample_score,
    JSONExtractFloat(snapshots.features_json, 'data_volume_score') AS data_volume_score
FROM
(
    SELECT
        address,
        argMax(snapshot_id, generated_at_unix_ms) AS snapshot_id,
        argMax(features_json, generated_at_unix_ms) AS features_json,
        max(generated_at_unix_ms) AS generated_at_unix_ms
    FROM tron_db.wallet_ml_feature_snapshots
    WHERE feature_schema_version = 'tron_wallet_behavior_features_v2'
    GROUP BY address
) AS snapshots
INNER JOIN
(
    SELECT
        address,
        argMax(label, created_at_unix_ms) AS label,
        argMax(source, created_at_unix_ms) AS label_source,
        argMax(case_id, created_at_unix_ms) AS case_id,
        max(created_at_unix_ms) AS label_created_at_unix_ms
    FROM tron_db.wallet_ml_labels
    GROUP BY address
) AS labels
    ON labels.address = snapshots.address
ORDER BY snapshots.address;
