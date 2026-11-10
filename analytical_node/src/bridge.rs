use crate::{App, Failure, internal, now, owner, response};
use aml_chain_api::{
    Network,
    bridge::{self, Leg, Log, Observation, Side, WINDOW_MS, Window},
    kyt::transaction_hash,
};
use anyhow::{Context, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use num_bigint::BigUint;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

#[path = "bridge_allbridge.rs"]
mod allbridge;
#[path = "bridge_tbtc.rs"]
mod tbtc;

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;

const POLICY: &str = "celer_pool_v2_match_v1";
const AMOUNT_TOLERANCE_BPS: u32 = 500;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/investigations/{id}/bridges", get(list).post(trace))
        .layer(DefaultBodyLimit::max(2048))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    tx_hash: String,
    log_index: Option<u32>,
    output_index: Option<u32>,
    #[serde(default)]
    verify_sweep: bool,
    #[serde(default)]
    verify_mint: bool,
}

fn missing() -> Failure {
    Failure(StatusCode::NOT_FOUND, "Investigation not found or expired")
}

async fn list(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let owner = owner(&app, &headers)?;
    let traces = app
        .store
        .bridge_traces(&id, &owner.hash, now())
        .await
        .map_err(internal)?
        .ok_or_else(missing)?;
    Ok(response(json!({"traces": traces}), owner))
}

async fn trace(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Input>,
) -> Result<Response, Failure> {
    let owner = owner(&app, &headers)?;
    let investigation = app
        .store
        .read(&id, &owner.hash, now())
        .await
        .map_err(internal)?
        .ok_or_else(missing)?;
    let network = Network::parse(
        investigation["investigation"]["network_id"]
            .as_str()
            .unwrap_or_default(),
    )
    .map_err(internal)?;
    if network != Network::Bitcoin
        && bridge::pool(network).is_none()
        && bridge::allbridge::emitter(network).is_none()
    {
        return Ok(response(json!({"status": "unsupported_network"}), owner));
    }
    if (network == Network::Bitcoin && input.log_index.is_some())
        || (network != Network::Bitcoin && input.output_index.is_some())
    {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "Index does not apply to this network",
        ));
    }
    if network != Network::Bitcoin && (input.verify_sweep || input.verify_mint) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "tBTC verification requires a Bitcoin investigation",
        ));
    }
    let stage = match (input.verify_sweep, input.verify_mint) {
        (false, false) => tbtc::Stage::Reveal,
        (true, false) => tbtc::Stage::Sweep,
        (false, true) => tbtc::Stage::Mint,
        (true, true) => {
            return Err(Failure(
                StatusCode::BAD_REQUEST,
                "Select one tBTC verification",
            ));
        }
    };
    let hash = transaction_hash(network, &input.tx_hash)
        .map_err(|_| Failure(StatusCode::BAD_REQUEST, "Invalid transaction hash"))?;
    let graph = investigation.get("graph").unwrap_or(&investigation);
    let blocks = [
        graph["edges"].as_array(),
        investigation["semantic_events"].as_array(),
        investigation["activity"]["recent_semantic_events"].as_array(),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .filter(|row| {
        row["tx_hash"]
            .as_str()
            .and_then(|tx| transaction_hash(network, tx).ok())
            .as_deref()
            == Some(hash.as_str())
    })
    .filter_map(|row| row["block_number"].as_u64())
    .collect::<BTreeSet<_>>();
    let block = match blocks.len() {
        0 => {
            return Err(Failure(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Transaction is outside this investigation",
            ));
        }
        1 => *blocks.first().expect("one observed block"),
        _ => return Ok(response(json!({"status":"ambiguous_anchor"}), owner)),
    };
    let _permit = app.requests.try_acquire().map_err(|_| {
        Failure(
            StatusCode::TOO_MANY_REQUESTS,
            "Investigation capacity reached; retry shortly",
        )
    })?;
    let verified = if network == Network::Bitcoin {
        tbtc::verify(&app, &hash, block, input.output_index, stage).await
    } else {
        verify(&app, network, &hash, block, input.log_index).await
    };
    let result = match verified.map_err(internal)? {
        Ok(trace) => {
            let saved = app
                .store
                .record_bridge(&id, &owner.hash, now(), trace)
                .await
                .map_err(internal)?
                .ok_or(Failure(
                    StatusCode::CONFLICT,
                    "Investigation expired or bridge trace limit reached",
                ))?;
            json!({"status": "matched", "trace": saved})
        }
        Err(status) => json!({"status": status}),
    };
    Ok(response(result, owner))
}

async fn verify(
    app: &App,
    network: Network,
    hash: &str,
    block_number: u64,
    log_index: Option<u32>,
) -> anyhow::Result<Result<Value, &'static str>> {
    let anchor: Observation = get_json(
        app,
        network,
        &format!("/internal/bridge/transactions/{hash}?block_number={block_number}"),
        None,
    )
    .await?;
    validate_observation(network, &anchor)?;
    ensure!(
        anchor
            .logs
            .iter()
            .all(|log| log.tx_hash == hash && log.block_number == block_number),
        "wrong anchor transaction"
    );
    if anchor.truncated {
        return Ok(Err("ambiguous_anchor"));
    }
    let logs: Vec<_> = anchor
        .logs
        .iter()
        .filter(|log| log_index.is_none_or(|index| log.log_index == index))
        .collect();
    let log = match logs.as_slice() {
        [] => return Ok(Err("anchor_not_found")),
        [log] => *log,
        _ => return Ok(Err("ambiguous_anchor")),
    };
    if bridge::allbridge::emitter(network) == Some(log.address.as_str()) {
        return allbridge::verify(app, network, &anchor, log).await;
    }
    let leg = log
        .decode(network)
        .context("invalid anchor bridge receipt")?;
    let other = match (network, leg.remote_chain_id) {
        (Network::Ethereum, 56) => Network::Bsc,
        (Network::Bsc, 1) => Network::Ethereum,
        _ => return Ok(Err("unsupported_route")),
    };
    if asset(network, &leg.token).is_none() {
        return Ok(Err("unsupported_asset"));
    }
    let window = match leg.side {
        Side::Send => Window {
            side: Side::Relay,
            from_ms: log.timestamp_ms,
            to_ms: log.timestamp_ms.saturating_add(WINDOW_MS),
        },
        Side::Relay => Window {
            side: Side::Send,
            from_ms: log.timestamp_ms.saturating_sub(WINDOW_MS),
            to_ms: log.timestamp_ms,
        },
    };
    let peer: Observation = get_json(
        app,
        other,
        &format!("/internal/bridge/operations/{}", leg.operation_id),
        Some(&window),
    )
    .await?;
    validate_observation(other, &peer)?;
    if peer.truncated || peer.logs.len() > 1 {
        return Ok(Err("ambiguous_peer"));
    }
    let Some(peer_log) = peer.logs.first() else {
        return Ok(Err("peer_not_found_in_indexed_window"));
    };
    let peer_leg = peer_log
        .decode(other)
        .context("invalid peer bridge receipt")?;
    let (from, to) = match leg.side {
        Side::Send => (
            (network, log, &leg, &anchor.source_snapshot),
            (other, peer_log, &peer_leg, &peer.source_snapshot),
        ),
        Side::Relay => (
            (other, peer_log, &peer_leg, &peer.source_snapshot),
            (network, log, &leg, &anchor.source_snapshot),
        ),
    };
    let Some(pair) = matched_pair(from, to) else {
        return Ok(Err("inconsistent_pair"));
    };
    finish(
        app,
        (network, &anchor.source_snapshot),
        (other, &peer.source_snapshot),
        pair,
    )
    .await
}

async fn finish(
    app: &App,
    (network, anchor): (Network, &Value),
    (other, peer): (Network, &Value),
    pair: Value,
) -> anyhow::Result<Result<Value, &'static str>> {
    let (anchor_status, peer_status) = tokio::try_join!(
        get_json::<Value>(app, network, bridge::SNAPSHOT_ROUTE, None),
        get_json::<Value>(app, other, bridge::SNAPSHOT_ROUTE, None)
    )?;
    if &anchor_status != anchor || &peer_status != peer {
        return Ok(Err("source_changed"));
    }
    Ok(Ok(pair))
}

fn validate_observation(network: Network, observation: &Observation) -> anyhow::Result<()> {
    let snapshot = &observation.source_snapshot;
    validate_snapshot(network, &observation.network_id, snapshot)?;
    ensure!(
        observation.logs.len() <= bridge::LOG_LIMIT,
        "bridge log limit exceeded"
    );
    for log in &observation.logs {
        validate_occurrence(
            snapshot,
            log.block_number,
            &log.block_hash,
            log.timestamp_ms,
        )?;
    }
    Ok(())
}

fn validate_snapshot(network: Network, network_id: &str, snapshot: &Value) -> anyhow::Result<()> {
    ensure!(
        network_id == network.id()
            && snapshot["network_id"] == network.id()
            && snapshot["contract"] == "canonical_v1"
            && snapshot["snapshot_id"]
                .as_str()
                .is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())),
        "invalid bridge source observation",
    );
    Ok(())
}

fn validate_occurrence(
    snapshot: &Value,
    block: u64,
    hash: &str,
    timestamp: u64,
) -> anyhow::Result<()> {
    let start = snapshot["start_block"]
        .as_u64()
        .context("missing source start")?;
    let tip = snapshot["as_of_block"]
        .as_u64()
        .context("missing source tip")?;
    ensure!(
        snapshot["initialized"] == true
            && (start..=tip).contains(&block)
            && (block != tip || snapshot["block_hash"] == hash)
            && timestamp > 0,
        "bridge evidence outside canonical snapshot",
    );
    Ok(())
}

async fn get_json<T: DeserializeOwned>(
    app: &App,
    network: Network,
    path: &str,
    window: Option<&Window>,
) -> anyhow::Result<T> {
    let upstream = app
        .upstreams
        .get(network.name())
        .context("missing bridge upstream")?;
    let mut request = app
        .http
        .get(format!("{upstream}{path}"))
        .header("x-aml-service-key", &app.key)
        .timeout(Duration::from_secs(10));
    if let Some(window) = window {
        request = request.query(window);
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| error.without_url())?
        .error_for_status()
        .map_err(|error| error.without_url())?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| error.without_url())?
    {
        ensure!(
            bytes.len() + chunk.len() <= 65536,
            "bridge response exceeds bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

// Celer's documented token addresses and decimals for this corridor.
fn asset(network: Network, token: &str) -> Option<(&'static str, u32)> {
    match (network, token) {
        (Network::Ethereum, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48") => Some(("USDC", 6)),
        (Network::Bsc, "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d") => Some(("USDC", 18)),
        (Network::Ethereum, "0xdac17f958d2ee523a2206206994597c13d831ec7") => Some(("USDT", 6)),
        (Network::Bsc, "0x55d398326f99059ff775485246999027b3197955") => Some(("USDT", 18)),
        _ => None,
    }
}

type Evidence<'a> = (Network, &'a Log, &'a Leg, &'a Value);

fn matched_pair(from: Evidence<'_>, to: Evidence<'_>) -> Option<Value> {
    let (from_network, send, source, _) = from;
    let (to_network, relay, destination, _) = to;
    let corridor = match (from_network, to_network) {
        (Network::Ethereum, Network::Bsc) => (1, 56),
        (Network::Bsc, Network::Ethereum) => (56, 1),
        _ => return None,
    };
    let delay = relay.timestamp_ms.checked_sub(send.timestamp_ms)?;
    if source.side != Side::Send
        || destination.side != Side::Relay
        || (destination.remote_chain_id, source.remote_chain_id) != corridor
        || source.operation_id != destination.operation_id
        || source.sender != destination.sender
        || source.receiver != destination.receiver
        || delay > WINDOW_MS
    {
        return None;
    }
    let (symbol, source_decimals) = asset(from_network, &source.token)?;
    let (destination_symbol, destination_decimals) = asset(to_network, &destination.token)?;
    let sent = BigUint::parse_bytes(source.amount_word.as_bytes(), 16)?;
    let received = BigUint::parse_bytes(destination.amount_word.as_bytes(), 16)?;
    let decimals = source_decimals.max(destination_decimals);
    let base = BigUint::from(10u8);
    let normalized_send = &sent * base.pow(decimals - source_decimals);
    let normalized_relay = &received * base.pow(decimals - destination_decimals);
    let relay_bps = normalized_relay * 10000u32;
    if symbol != destination_symbol
        || relay_bps < &normalized_send * (10000 - AMOUNT_TOLERANCE_BPS)
        || relay_bps > &normalized_send * (10000 + AMOUNT_TOLERANCE_BPS)
    {
        return None;
    }
    let evidence = |(network, log, leg, snapshot): Evidence<'_>, amount: BigUint, decimals| {
        json!({"network_id":network.id(),"receipt":log,"source_snapshot":snapshot,
            "event":leg.side,"event_id":leg.event_id,"token_address":leg.token,
            "amount_raw":amount.to_string(),"decimals":decimals})
    };
    Some(json!({
        "policy": POLICY, "protocol": "celer_cbridge_pool_v2", "claim": "protocol_operation",
        "confidence": "high", "validity": "historical_capture", "operation_id":source.operation_id,
        "sender":source.sender,"receiver":source.receiver,"asset":symbol,"delay_ms":delay,
        "amount_tolerance_bps":AMOUNT_TOLERANCE_BPS,"window_ms":WINDOW_MS,
        "captured_at_unix_ms":now(),"settlement_verified":false,"fund_continuity_claimed":false,
        "send":evidence(from, sent, source_decimals),"relay":evidence(to, received, destination_decimals)
    }))
}
