use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use chrono::{TimeZone, Timelike, Utc};
use clickhouse::Client;
use serde::{Deserialize, Serialize};

use crate::services::tron::observation_window::{normalize_window_days, window_start_unix_ms};

#[derive(Debug, Clone, Serialize)]
pub struct WalletFingerprint {
    pub address: String,
    pub window_days: u16,
    pub sampled_event_limit: u64,
    pub is_truncated: bool,
    pub fingerprint_label: String,
    pub wallet_type: String,
    pub confidence: f32,
    pub identity: WalletIdentity,
    pub flows: WalletFlowSummary,
    pub behavior: WalletBehaviorSummary,
    pub dominant_tokens: Vec<TokenUsage>,
    pub senders: Vec<WalletCounterpartyFingerprint>,
    pub receivers: Vec<WalletCounterpartyFingerprint>,
    pub behavior_flags: Vec<String>,
    pub evidence: Vec<String>,
    pub generated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletIdentity {
    pub address: String,
    pub identity_type: String,
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub exchange_name: Option<String>,
    pub exchange_role: Option<String>,
    pub confidence: f32,
    pub source: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletFlowSummary {
    pub total_transfers: u64,
    pub unique_transactions: u64,
    pub incoming_transfers: u64,
    pub outgoing_transfers: u64,
    pub unique_senders: u64,
    pub unique_receivers: u64,
    pub total_volume_in_raw: String,
    pub total_volume_out_raw: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletBehaviorSummary {
    pub first_seen_timestamp: Option<u64>,
    pub last_seen_timestamp: Option<u64>,
    pub observed_days: f64,
    pub active_days: u64,
    pub active_hours: Vec<u8>,
    pub avg_tx_interval_seconds: f64,
    pub burst_score: f32,
    pub inbound_outbound_ratio: f32,
    pub counterparty_concentration: f32,
    pub token_diversity: u32,
    pub contract_call_ratio: f32,
    pub swap_ratio: f32,
    pub bridge_ratio: f32,
    pub exchange_interaction_ratio: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsage {
    pub token_address: String,
    pub transfer_count: u64,
    pub unique_transactions: u64,
    pub volume_raw: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletCounterpartyFingerprint {
    pub address: String,
    pub direction: String,
    pub relationship_label: String,
    pub identity: WalletIdentity,
    pub transfer_count: u64,
    pub unique_transactions: u64,
    pub total_volume_raw: String,
    pub first_seen_timestamp: u64,
    pub last_seen_timestamp: u64,
    pub tokens: Vec<String>,
    pub dominant_token: Option<String>,
    pub share_of_wallet_transfers: f32,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct WalletEventRow {
    tx_hash: String,
    timestamp: u64,
    direction: String,
    counterparty: String,
    token_address: String,
    amount_raw: String,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct WalletAggregateRow {
    total_transfers: u64,
    unique_transactions: u64,
    incoming_transfers: u64,
    outgoing_transfers: u64,
    unique_senders: u64,
    unique_receivers: u64,
    total_volume_in_raw: String,
    total_volume_out_raw: String,
    first_seen_timestamp: u64,
    last_seen_timestamp: u64,
    active_days: u64,
    token_diversity: u64,
    swap_transfers: u64,
    bridge_transfers: u64,
    contract_call_transfers: u64,
    exchange_transfers: u64,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct ExchangeIdentityRow {
    entity_id: String,
    exchange_name: String,
    role: String,
    confidence: f32,
    source: String,
    #[serde(rename = "last_seen_block")]
    _last_seen_block: u64,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct EntityIdentityRow {
    entity_id: String,
    entity_name: String,
    entity_type: String,
    confidence: f32,
    source: String,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct ContractIdentityRow {
    protocol_name: String,
    contract_type: String,
}

#[derive(Debug, Clone)]
struct CounterpartyAggregate {
    address: String,
    direction: String,
    transfer_count: u64,
    tx_hashes: HashSet<String>,
    total_volume: u128,
    first_seen_timestamp: u64,
    last_seen_timestamp: u64,
    tokens: BTreeSet<String>,
    token_counts: BTreeMap<String, u64>,
}

pub async fn build_wallet_fingerprint(
    clickhouse: Arc<Client>,
    address: &str,
    window_days: Option<u16>,
    top_counterparties: Option<usize>,
    max_events: Option<u64>,
) -> anyhow::Result<WalletFingerprint> {
    let window_days = normalize_window_days(window_days);
    let top_counterparties = top_counterparties.unwrap_or(25).clamp(1, 100);
    let sampled_event_limit = max_events.unwrap_or(20_000).clamp(100, 250_000);
    let generated_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let window_start_ms = window_start_unix_ms(generated_at_unix_ms, window_days);

    let identity = load_wallet_identity(&clickhouse, address).await?;
    let aggregate = load_wallet_aggregate(&clickhouse, address, window_start_ms).await?;
    let events =
        load_wallet_events(&clickhouse, address, window_start_ms, sampled_event_limit).await?;
    let is_truncated = aggregate.total_transfers > events.len() as u64;

    if events.is_empty() {
        return Ok(empty_fingerprint(
            address,
            window_days,
            sampled_event_limit,
            identity,
            generated_at_unix_ms,
        ));
    }

    let flows = build_flow_summary(&aggregate);
    let behavior = build_behavior_summary(&events, &flows, &aggregate);
    let dominant_tokens = build_token_usage(&events);

    let counterparties = build_counterparty_aggregates(&events);
    let senders = build_counterparty_fingerprints(
        &clickhouse,
        counterparties
            .iter()
            .filter(|counterparty| counterparty.direction == "sender")
            .collect(),
        top_counterparties,
        flows.total_transfers,
    )
    .await?;
    let receivers = build_counterparty_fingerprints(
        &clickhouse,
        counterparties
            .iter()
            .filter(|counterparty| counterparty.direction == "receiver")
            .collect(),
        top_counterparties,
        flows.total_transfers,
    )
    .await?;

    let (wallet_type, fingerprint_label, confidence, evidence) =
        classify_wallet(&identity, &flows, &behavior);
    let behavior_flags = build_behavior_flags(&flows, &behavior);

    Ok(WalletFingerprint {
        address: address.to_string(),
        window_days,
        sampled_event_limit,
        is_truncated,
        fingerprint_label,
        wallet_type,
        confidence,
        identity,
        flows,
        behavior,
        dominant_tokens,
        senders,
        receivers,
        behavior_flags,
        evidence,
        generated_at_unix_ms,
    })
}

async fn load_wallet_aggregate(
    clickhouse: &Client,
    address: &str,
    window_start_ms: u64,
) -> anyhow::Result<WalletAggregateRow> {
    clickhouse
        .query(
            r#"
            SELECT
                count() AS total_transfers,
                uniqExact(ar.tx_hash) AS unique_transactions,
                countIf(ar.to_address = ?) AS incoming_transfers,
                countIf(ar.from_address = ?) AS outgoing_transfers,
                uniqExactIf(ar.from_address, ar.to_address = ?) AS unique_senders,
                uniqExactIf(ar.to_address, ar.from_address = ?) AS unique_receivers,
                toString(sumIf(ar.amount, ar.to_address = ?)) AS total_volume_in_raw,
                toString(sumIf(ar.amount, ar.from_address = ?)) AS total_volume_out_raw,
                minOrNull(ar.timestamp) AS first_seen_timestamp_nullable,
                maxOrNull(ar.timestamp) AS last_seen_timestamp_nullable,
                uniqExact(intDiv(ar.timestamp, 86400000)) AS active_days,
                uniqExact(ar.token_address) AS token_diversity,
                countIf(ifNull(tf.is_swap, toUInt8(0)) > 0) AS swap_transfers,
                countIf(ifNull(tf.is_bridge, toUInt8(0)) > 0) AS bridge_transfers,
                countIf(ifNull(tf.is_contract_call, toUInt8(0)) > 0) AS contract_call_transfers,
                countIf(ifNull(ex.confidence, toFloat32(0)) > 0) AS exchange_transfers
            FROM address_relationships_canonical AS ar
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    max(is_swap) AS is_swap,
                    max(is_bridge) AS is_bridge,
                    max(is_contract_call) AS is_contract_call
                FROM transaction_features
                GROUP BY tx_hash
            ) AS tf ON tf.tx_hash = ar.tx_hash
            LEFT JOIN
            (
                SELECT
                    address,
                    max(confidence) AS confidence
                FROM exchange_addresses FINAL
                WHERE is_active = 1
                GROUP BY address
            ) AS ex
                ON ex.address = if(ar.from_address = ?, ar.to_address, ar.from_address)
            WHERE (ar.from_address = ? OR ar.to_address = ?)
              AND ar.timestamp >= ?
            "#,
        )
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(window_start_ms)
        .fetch_one::<WalletAggregateNullableRow>()
        .await
        .map(WalletAggregateRow::from)
        .context("failed to load full-window TRON wallet aggregates")
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct WalletAggregateNullableRow {
    total_transfers: u64,
    unique_transactions: u64,
    incoming_transfers: u64,
    outgoing_transfers: u64,
    unique_senders: u64,
    unique_receivers: u64,
    total_volume_in_raw: String,
    total_volume_out_raw: String,
    first_seen_timestamp_nullable: Option<u64>,
    last_seen_timestamp_nullable: Option<u64>,
    active_days: u64,
    token_diversity: u64,
    swap_transfers: u64,
    bridge_transfers: u64,
    contract_call_transfers: u64,
    exchange_transfers: u64,
}

impl From<WalletAggregateNullableRow> for WalletAggregateRow {
    fn from(row: WalletAggregateNullableRow) -> Self {
        Self {
            total_transfers: row.total_transfers,
            unique_transactions: row.unique_transactions,
            incoming_transfers: row.incoming_transfers,
            outgoing_transfers: row.outgoing_transfers,
            unique_senders: row.unique_senders,
            unique_receivers: row.unique_receivers,
            total_volume_in_raw: row.total_volume_in_raw,
            total_volume_out_raw: row.total_volume_out_raw,
            first_seen_timestamp: row.first_seen_timestamp_nullable.unwrap_or_default(),
            last_seen_timestamp: row.last_seen_timestamp_nullable.unwrap_or_default(),
            active_days: row.active_days,
            token_diversity: row.token_diversity,
            swap_transfers: row.swap_transfers,
            bridge_transfers: row.bridge_transfers,
            contract_call_transfers: row.contract_call_transfers,
            exchange_transfers: row.exchange_transfers,
        }
    }
}

async fn load_wallet_events(
    clickhouse: &Client,
    address: &str,
    window_start_ms: u64,
    limit: u64,
) -> anyhow::Result<Vec<WalletEventRow>> {
    clickhouse
        .query(
            r#"
            SELECT
                ar.tx_hash AS tx_hash,
                ar.timestamp AS timestamp,
                if(ar.from_address = ?, 'out', 'in') AS direction,
                if(ar.from_address = ?, ar.to_address, ar.from_address) AS counterparty,
                ar.token_address AS token_address,
                toString(ar.amount) AS amount_raw
            FROM address_relationships_canonical AS ar
            WHERE (ar.from_address = ? OR ar.to_address = ?)
              AND ar.timestamp >= ?
            ORDER BY ar.timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(window_start_ms)
        .bind(limit)
        .fetch_all::<WalletEventRow>()
        .await
        .context("failed to load TRON wallet fingerprint events")
}

async fn load_wallet_identity(
    clickhouse: &Client,
    address: &str,
) -> anyhow::Result<WalletIdentity> {
    if let Some(exchange) = load_exchange_identity(clickhouse, address).await? {
        return Ok(exchange_identity(address, exchange));
    }

    if let Some(entity) = load_entity_identity(clickhouse, address).await? {
        return Ok(entity_identity(address, entity));
    }

    if let Some(contract) = load_contract_identity(clickhouse, address).await? {
        return Ok(contract_identity(address, contract));
    }

    Ok(WalletIdentity {
        address: address.to_string(),
        identity_type: "wallet".to_string(),
        entity_id: None,
        entity_name: None,
        entity_type: None,
        exchange_name: None,
        exchange_role: None,
        confidence: 0.25,
        source: "unlabeled".to_string(),
        tags: vec!["unlabeled_wallet".to_string()],
    })
}

async fn load_exchange_identity(
    clickhouse: &Client,
    address: &str,
) -> anyhow::Result<Option<ExchangeIdentityRow>> {
    clickhouse
        .query(
            r#"
            SELECT
                entity_id,
                exchange_name,
                role,
                confidence,
                source,
                last_seen_block
            FROM
            (
                SELECT
                    entity_id,
                    exchange_name,
                    address_role AS role,
                    confidence,
                    detection_source AS source,
                    last_seen_block
                FROM exchange_addresses FINAL
                WHERE address = ? AND is_active = 1
            )
            ORDER BY confidence DESC, last_seen_block DESC
            LIMIT 1
            "#,
        )
        .bind(address)
        .fetch_optional::<ExchangeIdentityRow>()
        .await
        .context("failed to load TRON exchange wallet identity")
}

async fn load_entity_identity(
    clickhouse: &Client,
    address: &str,
) -> anyhow::Result<Option<EntityIdentityRow>> {
    clickhouse
        .query(
            r#"
            SELECT
                entity_id,
                entity_name,
                entity_type,
                confidence,
                source
            FROM address_entity FINAL
            WHERE address = ?
              AND is_active = 1
            ORDER BY confidence DESC, created_at DESC
            LIMIT 1
            "#,
        )
        .bind(address)
        .fetch_optional::<EntityIdentityRow>()
        .await
        .context("failed to load TRON address entity identity")
}

async fn load_contract_identity(
    clickhouse: &Client,
    address: &str,
) -> anyhow::Result<Option<ContractIdentityRow>> {
    clickhouse
        .query(
            r#"
            SELECT
                argMaxIf(
                    ifNull(feature.protocol, ''),
                    tx.timestamp,
                    ifNull(feature.protocol, '') != ''
                ) AS protocol_name,
                argMax(tx.contract_type, tx.timestamp) AS contract_type
            FROM transactions_canonical AS tx
            LEFT JOIN
            (
                SELECT tx_hash, protocol
                FROM transaction_features FINAL
            ) AS feature ON feature.tx_hash = tx.tx_hash
            WHERE tx.contract_address = ?
            HAVING count() > 0
            "#,
        )
        .bind(address)
        .fetch_optional::<ContractIdentityRow>()
        .await
        .context("failed to load TRON contract identity")
}

fn exchange_identity(address: &str, row: ExchangeIdentityRow) -> WalletIdentity {
    let normalized_role = row.role.to_ascii_lowercase();
    let identity_type = if normalized_role == "deposit" {
        "exchange_deposit_wallet"
    } else {
        "exchange_service_wallet"
    };
    let entity_id = if row.entity_id.is_empty() {
        None
    } else {
        Some(row.entity_id)
    };

    WalletIdentity {
        address: address.to_string(),
        identity_type: identity_type.to_string(),
        entity_id,
        entity_name: Some(row.exchange_name.clone()),
        entity_type: Some("centralized_exchange".to_string()),
        exchange_name: Some(row.exchange_name),
        exchange_role: Some(row.role.clone()),
        confidence: row.confidence,
        source: row.source,
        tags: vec![
            "exchange".to_string(),
            format!("exchange_role:{normalized_role}"),
        ],
    }
}

fn entity_identity(address: &str, row: EntityIdentityRow) -> WalletIdentity {
    WalletIdentity {
        address: address.to_string(),
        identity_type: row.entity_type.clone(),
        entity_id: Some(row.entity_id),
        entity_name: Some(row.entity_name),
        entity_type: Some(row.entity_type),
        exchange_name: None,
        exchange_role: None,
        confidence: row.confidence,
        source: row.source,
        tags: vec!["entity_attributed".to_string()],
    }
}

fn contract_identity(address: &str, row: ContractIdentityRow) -> WalletIdentity {
    let tags = vec!["contract".to_string()];

    let entity_name = if row.protocol_name.is_empty() {
        None
    } else {
        Some(row.protocol_name)
    };

    WalletIdentity {
        address: address.to_string(),
        identity_type: format!("contract:{}", row.contract_type),
        entity_id: None,
        entity_name,
        entity_type: Some(row.contract_type),
        exchange_name: None,
        exchange_role: None,
        confidence: 0.65,
        source: "contract_metadata".to_string(),
        tags,
    }
}

fn build_flow_summary(aggregate: &WalletAggregateRow) -> WalletFlowSummary {
    WalletFlowSummary {
        total_transfers: aggregate.total_transfers,
        unique_transactions: aggregate.unique_transactions,
        incoming_transfers: aggregate.incoming_transfers,
        outgoing_transfers: aggregate.outgoing_transfers,
        unique_senders: aggregate.unique_senders,
        unique_receivers: aggregate.unique_receivers,
        total_volume_in_raw: aggregate.total_volume_in_raw.clone(),
        total_volume_out_raw: aggregate.total_volume_out_raw.clone(),
    }
}

fn build_behavior_summary(
    events: &[WalletEventRow],
    flows: &WalletFlowSummary,
    aggregate: &WalletAggregateRow,
) -> WalletBehaviorSummary {
    let mut tx_timestamps = HashMap::<&str, u64>::new();
    let mut active_hours = BTreeSet::<u8>::new();
    let mut counterparty_counts = HashMap::<&str, u64>::new();

    for event in events {
        tx_timestamps
            .entry(event.tx_hash.as_str())
            .and_modify(|timestamp| *timestamp = (*timestamp).min(event.timestamp))
            .or_insert(event.timestamp);

        if let Some(hour) = hour_from_timestamp_ms(event.timestamp) {
            active_hours.insert(hour);
        }
        *counterparty_counts
            .entry(event.counterparty.as_str())
            .or_default() += 1;
    }

    let mut timestamps = tx_timestamps.into_values().collect::<Vec<_>>();
    timestamps.sort_unstable();

    let first_seen_timestamp =
        (aggregate.first_seen_timestamp > 0).then_some(aggregate.first_seen_timestamp);
    let last_seen_timestamp =
        (aggregate.last_seen_timestamp > 0).then_some(aggregate.last_seen_timestamp);
    let observed_days = match (first_seen_timestamp, last_seen_timestamp) {
        (Some(first), Some(last)) if last > first => (last - first) as f64 / 86_400_000.0,
        (Some(_), Some(_)) => 1.0,
        _ => 0.0,
    };
    let largest_counterparty_count = counterparty_counts.into_values().max().unwrap_or_default();

    WalletBehaviorSummary {
        first_seen_timestamp,
        last_seen_timestamp,
        observed_days,
        active_days: aggregate.active_days,
        active_hours: active_hours.into_iter().collect(),
        avg_tx_interval_seconds: avg_interval_seconds(&timestamps),
        burst_score: burst_score(&timestamps),
        inbound_outbound_ratio: ratio(
            flows.incoming_transfers as f64,
            flows.outgoing_transfers.max(1) as f64,
        ) as f32,
        counterparty_concentration: ratio(
            largest_counterparty_count as f64,
            flows.total_transfers.max(1) as f64,
        ) as f32,
        token_diversity: aggregate.token_diversity.min(u64::from(u32::MAX)) as u32,
        contract_call_ratio: ratio(
            aggregate.contract_call_transfers as f64,
            flows.total_transfers as f64,
        ) as f32,
        swap_ratio: ratio(
            aggregate.swap_transfers as f64,
            flows.total_transfers as f64,
        ) as f32,
        bridge_ratio: ratio(
            aggregate.bridge_transfers as f64,
            flows.total_transfers as f64,
        ) as f32,
        exchange_interaction_ratio: ratio(
            aggregate.exchange_transfers as f64,
            flows.total_transfers as f64,
        ) as f32,
    }
}

fn build_token_usage(events: &[WalletEventRow]) -> Vec<TokenUsage> {
    #[derive(Default)]
    struct TokenAggregate {
        transfer_count: u64,
        tx_hashes: HashSet<String>,
        volume: u128,
    }

    let mut aggregates = BTreeMap::<String, TokenAggregate>::new();

    for event in events {
        let aggregate = aggregates.entry(event.token_address.clone()).or_default();
        aggregate.transfer_count += 1;
        aggregate.tx_hashes.insert(event.tx_hash.clone());
        aggregate.volume = aggregate
            .volume
            .saturating_add(parse_amount(&event.amount_raw));
    }

    let mut usage = aggregates
        .into_iter()
        .map(|(token_address, aggregate)| TokenUsage {
            token_address,
            transfer_count: aggregate.transfer_count,
            unique_transactions: aggregate.tx_hashes.len() as u64,
            volume_raw: aggregate.volume.to_string(),
        })
        .collect::<Vec<_>>();

    usage.sort_by(|left, right| {
        right
            .transfer_count
            .cmp(&left.transfer_count)
            .then_with(|| left.token_address.cmp(&right.token_address))
    });
    usage.truncate(10);
    usage
}

fn build_counterparty_aggregates(events: &[WalletEventRow]) -> Vec<CounterpartyAggregate> {
    let mut aggregates = HashMap::<(String, String), CounterpartyAggregate>::new();

    for event in events {
        let direction = if event.direction == "in" {
            "sender"
        } else {
            "receiver"
        };
        let key = (event.counterparty.clone(), direction.to_string());
        let aggregate = aggregates
            .entry(key)
            .or_insert_with(|| CounterpartyAggregate {
                address: event.counterparty.clone(),
                direction: direction.to_string(),
                transfer_count: 0,
                tx_hashes: HashSet::new(),
                total_volume: 0,
                first_seen_timestamp: event.timestamp,
                last_seen_timestamp: event.timestamp,
                tokens: BTreeSet::new(),
                token_counts: BTreeMap::new(),
            });

        aggregate.transfer_count += 1;
        aggregate.tx_hashes.insert(event.tx_hash.clone());
        aggregate.total_volume = aggregate
            .total_volume
            .saturating_add(parse_amount(&event.amount_raw));
        aggregate.first_seen_timestamp = aggregate.first_seen_timestamp.min(event.timestamp);
        aggregate.last_seen_timestamp = aggregate.last_seen_timestamp.max(event.timestamp);
        aggregate.tokens.insert(event.token_address.clone());
        *aggregate
            .token_counts
            .entry(event.token_address.clone())
            .or_default() += 1;
    }

    let mut aggregates = aggregates.into_values().collect::<Vec<_>>();
    aggregates.sort_by(|left, right| {
        right
            .transfer_count
            .cmp(&left.transfer_count)
            .then_with(|| right.tx_hashes.len().cmp(&left.tx_hashes.len()))
    });
    aggregates
}

async fn build_counterparty_fingerprints(
    clickhouse: &Client,
    counterparties: Vec<&CounterpartyAggregate>,
    limit: usize,
    total_wallet_transfers: u64,
) -> anyhow::Result<Vec<WalletCounterpartyFingerprint>> {
    let mut rows = Vec::new();

    for aggregate in counterparties.into_iter().take(limit) {
        let identity = load_wallet_identity(clickhouse, &aggregate.address).await?;
        let dominant_token = aggregate
            .token_counts
            .iter()
            .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
            .map(|(token, _)| token.clone());

        rows.push(WalletCounterpartyFingerprint {
            address: aggregate.address.clone(),
            direction: aggregate.direction.clone(),
            relationship_label: relationship_label(&aggregate.direction, &identity),
            identity,
            transfer_count: aggregate.transfer_count,
            unique_transactions: aggregate.tx_hashes.len() as u64,
            total_volume_raw: aggregate.total_volume.to_string(),
            first_seen_timestamp: aggregate.first_seen_timestamp,
            last_seen_timestamp: aggregate.last_seen_timestamp,
            tokens: aggregate.tokens.iter().cloned().collect(),
            dominant_token,
            share_of_wallet_transfers: ratio(
                aggregate.transfer_count as f64,
                total_wallet_transfers.max(1) as f64,
            ) as f32,
        });
    }

    Ok(rows)
}

fn relationship_label(direction: &str, identity: &WalletIdentity) -> String {
    if identity.identity_type.starts_with("exchange") {
        return match direction {
            "sender" => "exchange funding source",
            _ => "exchange cash-out destination",
        }
        .to_string();
    }

    if identity.identity_type.contains("contract") {
        return "contract interaction".to_string();
    }

    if identity.identity_type.contains("sweeper") {
        return "sweeper wallet".to_string();
    }

    match direction {
        "sender" => "direct sender wallet",
        _ => "direct receiver wallet",
    }
    .to_string()
}

fn classify_wallet(
    identity: &WalletIdentity,
    flows: &WalletFlowSummary,
    behavior: &WalletBehaviorSummary,
) -> (String, String, f32, Vec<String>) {
    let mut evidence = Vec::new();
    let evidence_confidence = evidence_confidence(flows, behavior);

    if identity.identity_type.starts_with("exchange") {
        evidence.push(format!(
            "identity_source={} confidence={:.2}",
            identity.source, identity.confidence
        ));
        return (
            identity.identity_type.clone(),
            "Identified exchange wallet".to_string(),
            identity.confidence.max(evidence_confidence),
            evidence,
        );
    }

    if behavior.exchange_interaction_ratio >= 0.35
        && flows.unique_senders >= 10
        && flows.unique_receivers <= 5
        && flows.incoming_transfers > flows.outgoing_transfers
    {
        evidence.push("many senders consolidate toward exchange-linked flows".to_string());
        return (
            "exchange_deposit_funnel".to_string(),
            "Probable exchange deposit funnel".to_string(),
            evidence_confidence.max(0.72),
            evidence,
        );
    }

    if flows.unique_senders >= 50 && flows.unique_receivers >= 50 && flows.total_transfers >= 200 {
        evidence.push("high fan-in and high fan-out service pattern".to_string());
        return (
            "service_or_exchange_hub".to_string(),
            "High-volume service or exchange hub".to_string(),
            evidence_confidence.max(0.7),
            evidence,
        );
    }

    if flows.unique_senders >= 10 && flows.unique_receivers <= 3 {
        evidence.push("many unique senders and few receivers".to_string());
        return (
            "collector_wallet".to_string(),
            "Collector wallet".to_string(),
            evidence_confidence.max(0.65),
            evidence,
        );
    }

    if flows.unique_receivers >= 10 && flows.unique_senders <= 3 {
        evidence.push("few senders and many unique receivers".to_string());
        return (
            "distributor_wallet".to_string(),
            "Distributor wallet".to_string(),
            evidence_confidence.max(0.65),
            evidence,
        );
    }

    if behavior.swap_ratio >= 0.25 {
        evidence.push(format!("swap_ratio={:.2}", behavior.swap_ratio));
        return (
            "defi_swapper".to_string(),
            "DeFi swap-heavy wallet".to_string(),
            evidence_confidence.max(0.62),
            evidence,
        );
    }

    if behavior.bridge_ratio >= 0.15 {
        evidence.push(format!("bridge_ratio={:.2}", behavior.bridge_ratio));
        return (
            "bridge_user".to_string(),
            "Bridge-heavy wallet".to_string(),
            evidence_confidence.max(0.62),
            evidence,
        );
    }

    if behavior.contract_call_ratio >= 0.50 {
        evidence.push(format!(
            "contract_call_ratio={:.2}",
            behavior.contract_call_ratio
        ));
        return (
            "contract_power_user".to_string(),
            "Contract-heavy wallet".to_string(),
            evidence_confidence.max(0.6),
            evidence,
        );
    }

    if behavior.burst_score >= 0.75 && flows.unique_receivers >= 5 {
        evidence.push(format!("burst_score={:.2}", behavior.burst_score));
        return (
            "burst_distributor".to_string(),
            "Burst distribution wallet".to_string(),
            evidence_confidence.max(0.58),
            evidence,
        );
    }

    evidence.push("low-to-moderate activity without strong service pattern".to_string());
    (
        "retail_wallet".to_string(),
        "Ordinary wallet behavior".to_string(),
        evidence_confidence,
        evidence,
    )
}

fn build_behavior_flags(
    flows: &WalletFlowSummary,
    behavior: &WalletBehaviorSummary,
) -> Vec<String> {
    let mut flags = Vec::new();

    if behavior.exchange_interaction_ratio >= 0.5 {
        flags.push("exchange_interaction_heavy".to_string());
    }
    if behavior.burst_score >= 0.75 {
        flags.push("bursty_activity".to_string());
    }
    if behavior.counterparty_concentration >= 0.75 && flows.total_transfers >= 5 {
        flags.push("counterparty_concentration".to_string());
    }
    if flows.unique_senders >= 25 && flows.unique_receivers <= 3 {
        flags.push("many_senders_few_receivers".to_string());
    }
    if flows.unique_receivers >= 25 && flows.unique_senders <= 3 {
        flags.push("few_senders_many_receivers".to_string());
    }
    if behavior.bridge_ratio >= 0.25 {
        flags.push("bridge_heavy_activity".to_string());
    }
    if behavior.swap_ratio >= 0.4 {
        flags.push("swap_heavy_activity".to_string());
    }

    flags
}

fn evidence_confidence(flows: &WalletFlowSummary, behavior: &WalletBehaviorSummary) -> f32 {
    let volume_confidence = if flows.total_transfers >= 200 {
        0.85
    } else if flows.total_transfers >= 50 {
        0.75
    } else if flows.total_transfers >= 10 {
        0.62
    } else if flows.total_transfers >= 3 {
        0.45
    } else {
        0.30
    };

    let age_bonus = if behavior.observed_days >= 30.0 {
        0.08
    } else if behavior.observed_days >= 7.0 {
        0.04
    } else {
        0.0
    };

    clamp01(volume_confidence + age_bonus)
}

fn empty_fingerprint(
    address: &str,
    window_days: u16,
    sampled_event_limit: u64,
    identity: WalletIdentity,
    generated_at_unix_ms: u64,
) -> WalletFingerprint {
    WalletFingerprint {
        address: address.to_string(),
        window_days,
        sampled_event_limit,
        is_truncated: false,
        fingerprint_label: "No observed TRON flow history".to_string(),
        wallet_type: identity.identity_type.clone(),
        confidence: identity.confidence,
        identity,
        flows: WalletFlowSummary {
            total_transfers: 0,
            unique_transactions: 0,
            incoming_transfers: 0,
            outgoing_transfers: 0,
            unique_senders: 0,
            unique_receivers: 0,
            total_volume_in_raw: "0".to_string(),
            total_volume_out_raw: "0".to_string(),
        },
        behavior: WalletBehaviorSummary {
            first_seen_timestamp: None,
            last_seen_timestamp: None,
            observed_days: 0.0,
            active_days: 0,
            active_hours: Vec::new(),
            avg_tx_interval_seconds: 0.0,
            burst_score: 0.0,
            inbound_outbound_ratio: 0.0,
            counterparty_concentration: 0.0,
            token_diversity: 0,
            contract_call_ratio: 0.0,
            swap_ratio: 0.0,
            bridge_ratio: 0.0,
            exchange_interaction_ratio: 0.0,
        },
        dominant_tokens: Vec::new(),
        senders: Vec::new(),
        receivers: Vec::new(),
        behavior_flags: Vec::new(),
        evidence: vec!["no address_relationships rows in selected window".to_string()],
        generated_at_unix_ms,
    }
}

fn avg_interval_seconds(timestamps: &[u64]) -> f64 {
    if timestamps.len() < 2 {
        return 0.0;
    }

    let total_delta_ms = timestamps
        .windows(2)
        .map(|window| window[1].saturating_sub(window[0]))
        .sum::<u64>();

    total_delta_ms as f64 / (timestamps.len() - 1) as f64 / 1_000.0
}

fn burst_score(timestamps: &[u64]) -> f32 {
    if timestamps.len() < 3 {
        return 0.0;
    }

    let mut buckets = HashMap::<u64, u64>::new();
    for timestamp in timestamps {
        *buckets.entry(timestamp / 3_600_000).or_default() += 1;
    }

    let max_bucket = buckets.values().copied().max().unwrap_or_default() as f64;
    let avg_bucket = timestamps.len() as f64 / buckets.len().max(1) as f64;

    clamp01(((max_bucket / avg_bucket) - 1.0) / 9.0)
}

fn hour_from_timestamp_ms(timestamp: u64) -> Option<u8> {
    Utc.timestamp_millis_opt(timestamp as i64)
        .single()
        .map(|dt| dt.hour() as u8)
}

fn parse_amount(amount_raw: &str) -> u128 {
    amount_raw.parse::<u128>().unwrap_or(u128::MAX)
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

fn clamp01(value: f64) -> f32 {
    value.clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(identity_type: &str) -> WalletIdentity {
        WalletIdentity {
            address: "TWallet".to_string(),
            identity_type: identity_type.to_string(),
            entity_id: None,
            entity_name: None,
            entity_type: None,
            exchange_name: None,
            exchange_role: None,
            confidence: 0.8,
            source: "test".to_string(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn classifies_many_senders_few_receivers_as_collector() {
        let flows = WalletFlowSummary {
            total_transfers: 30,
            unique_transactions: 30,
            incoming_transfers: 25,
            outgoing_transfers: 5,
            unique_senders: 12,
            unique_receivers: 2,
            total_volume_in_raw: "1000".to_string(),
            total_volume_out_raw: "900".to_string(),
        };
        let behavior = WalletBehaviorSummary {
            first_seen_timestamp: Some(0),
            last_seen_timestamp: Some(86_400_000),
            observed_days: 1.0,
            active_days: 1,
            active_hours: vec![1, 2],
            avg_tx_interval_seconds: 60.0,
            burst_score: 0.1,
            inbound_outbound_ratio: 5.0,
            counterparty_concentration: 0.2,
            token_diversity: 1,
            contract_call_ratio: 0.0,
            swap_ratio: 0.0,
            bridge_ratio: 0.0,
            exchange_interaction_ratio: 0.0,
        };

        let (wallet_type, label, _, _) = classify_wallet(&identity("wallet"), &flows, &behavior);

        assert_eq!(wallet_type, "collector_wallet");
        assert_eq!(label, "Collector wallet");
    }

    #[test]
    fn exchange_identity_wins_over_behavioral_classification() {
        let flows = WalletFlowSummary {
            total_transfers: 1,
            unique_transactions: 1,
            incoming_transfers: 1,
            outgoing_transfers: 0,
            unique_senders: 1,
            unique_receivers: 0,
            total_volume_in_raw: "100".to_string(),
            total_volume_out_raw: "0".to_string(),
        };
        let behavior = WalletBehaviorSummary {
            first_seen_timestamp: Some(0),
            last_seen_timestamp: Some(0),
            observed_days: 1.0,
            active_days: 1,
            active_hours: vec![0],
            avg_tx_interval_seconds: 0.0,
            burst_score: 0.0,
            inbound_outbound_ratio: 1.0,
            counterparty_concentration: 1.0,
            token_diversity: 1,
            contract_call_ratio: 0.0,
            swap_ratio: 0.0,
            bridge_ratio: 0.0,
            exchange_interaction_ratio: 0.0,
        };

        let (wallet_type, label, confidence, _) =
            classify_wallet(&identity("exchange_deposit_wallet"), &flows, &behavior);

        assert_eq!(wallet_type, "exchange_deposit_wallet");
        assert_eq!(label, "Identified exchange wallet");
        assert!(confidence >= 0.8);
    }
}
