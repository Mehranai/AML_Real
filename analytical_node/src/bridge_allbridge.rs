use super::{
    App, Log, Network, Observation, Side, WINDOW_MS, Window, finish, get_json, now,
    validate_observation,
};
use aml_chain_api::{bridge::allbridge as protocol, kyt::wallet_address};
use anyhow::Context;
use num_bigint::BigUint;
use protocol::{Direction, Leg};
use serde_json::{Value, json};
use sha3::{Digest, Keccak256};

const POLICY: &str = "allbridge_core_message_v1";

pub(super) async fn verify(
    app: &App,
    network: Network,
    anchor: &Observation,
    log: &Log,
) -> anyhow::Result<Result<Value, &'static str>> {
    let Some(leg) = protocol::decode(network, log) else {
        return Ok(Err("unsupported_route"));
    };
    let other = protocol::peer(network).context("unknown Allbridge peer")?;
    let local_id = protocol::network_id(network).context("unknown local Allbridge chain ID")?;
    let peer_id = protocol::network_id(other).context("unknown peer Allbridge chain ID")?;
    let (key, window) = match leg.direction {
        Direction::Send { destination, token } => {
            if destination != peer_id {
                return Ok(Err("unsupported_route"));
            }
            if protocol::destination_usdt(other).is_none_or(|(expected, _, _)| token != expected) {
                return Ok(Err("unsupported_asset"));
            }
            let key = message(network, local_id, destination, &leg, &token)
                .context("unknown source Allbridge emitter")?;
            (
                key,
                Window {
                    side: Side::Relay,
                    from_ms: log.timestamp_ms,
                    to_ms: log.timestamp_ms.saturating_add(WINDOW_MS),
                },
            )
        }
        Direction::Receive { message } => {
            if message[..2] != [peer_id, local_id] {
                return Ok(Err("unsupported_route"));
            }
            (
                leg.nonce,
                Window {
                    side: Side::Send,
                    from_ms: log.timestamp_ms.saturating_sub(WINDOW_MS),
                    to_ms: log.timestamp_ms,
                },
            )
        }
    };
    let peer: Observation = get_json(
        app,
        other,
        &format!("/internal/bridge/allbridge/operations/{}", hex(&key)),
        Some(&window),
    )
    .await?;
    validate_observation(other, &peer)?;
    if peer.truncated {
        return Ok(Err("ambiguous_peer"));
    }
    if peer.logs.is_empty() {
        return Ok(Err("peer_not_found_in_indexed_window"));
    }
    let anchor_evidence = (network, log, &leg, &anchor.source_snapshot);
    let mut matches = peer.logs.iter().filter_map(|log| {
        let leg = protocol::decode(other, log)?;
        let evidence = (other, log, &leg, &peer.source_snapshot);
        match window.side {
            Side::Relay => paired(anchor_evidence, evidence),
            Side::Send => paired(evidence, anchor_evidence),
        }
    });
    let Some(pair) = matches.next() else {
        return Ok(Err("inconsistent_pair"));
    };
    if matches.next().is_some() {
        return Ok(Err("ambiguous_peer"));
    }
    finish(
        app,
        (network, &anchor.source_snapshot),
        (other, &peer.source_snapshot),
        pair,
    )
    .await
}

// Bridge.hashMessage and HashUtils.hashWithSender. Chain IDs occupy the first
// two bytes after both hashes and the original source bridge is a padded address.
fn message(
    network: Network,
    source: u8,
    destination: u8,
    leg: &Leg,
    token: &[u8; 32],
) -> Option<[u8; 32]> {
    let mut chain = [0; 32];
    chain[31] = source;
    let mut hash = Keccak256::new();
    hash.update(leg.amount);
    hash.update(leg.recipient);
    hash.update(chain);
    hash.update(token);
    hash.update(leg.nonce);
    hash.update([protocol::MESSENGER]);
    let mut payload: [u8; 32] = hash.finalize().into();
    payload[..2].copy_from_slice(&[source, destination]);
    let mut hash = Keccak256::new();
    hash.update(payload);
    hash.update(protocol::emitter_word(network)?);
    let mut result: [u8; 32] = hash.finalize().into();
    result[..2].copy_from_slice(&[source, destination]);
    Some(result)
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(2 + bytes.len() * 2);
    result.push_str("0x");
    for byte in bytes {
        write!(result, "{byte:02x}").expect("writing to String");
    }
    result
}

type Evidence<'a> = (Network, &'a Log, &'a Leg, &'a Value);

fn paired(from: Evidence<'_>, to: Evidence<'_>) -> Option<Value> {
    let (source, send, send_leg, source_snapshot) = from;
    let (destination, receive, receive_leg, destination_snapshot) = to;
    let Direction::Send {
        destination: route,
        token,
    } = send_leg.direction
    else {
        return None;
    };
    let Direction::Receive { message: received } = receive_leg.direction else {
        return None;
    };
    let (expected_token, token_address, decimals) = protocol::destination_usdt(destination)?;
    let source_id = protocol::network_id(source)?;
    let destination_id = protocol::network_id(destination)?;
    let delay = receive.timestamp_ms.checked_sub(send.timestamp_ms)?;
    if protocol::peer(source)? != destination
        || route != destination_id
        || token != expected_token
        || send_leg.recipient != receive_leg.recipient
        || send_leg.nonce != receive_leg.nonce
        || delay > WINDOW_MS
        || message(source, source_id, destination_id, send_leg, &token)? != received
    {
        return None;
    }
    let address = hex(&send_leg.recipient[12..]);
    let address = if destination == Network::Tron {
        format!("41{}", &address[2..])
    } else {
        address
    };
    let recipient = wallet_address(destination, &address).ok()?;
    Some(json!({
        "policy":POLICY,"protocol":"allbridge_core","claim":"protocol_operation","confidence":"high",
        "validity":"historical_capture","operation_id":hex(&received),"receiver":recipient,
        "asset":"USDT","asset_scope":"destination","delay_ms":delay,"window_ms":WINDOW_MS,
        "captured_at_unix_ms":now(),"settlement_verified":false,"fund_continuity_claimed":false,
        "send":{"network_id":source.id(),"receipt":send,"source_snapshot":source_snapshot,"event":"tokens_sent",
            "amount_raw":BigUint::from_bytes_be(&send_leg.amount).to_string(),"amount_unit":"protocol_vusd"},
        "relay":{"network_id":destination.id(),"receipt":receive,"source_snapshot":destination_snapshot,"event":"tokens_received",
            "token_address":token_address,"decimals":decimals,"amount_raw":BigUint::from_bytes_be(&receive_leg.amount).to_string()}
    }))
}

#[cfg(test)]
#[path = "bridge_allbridge_tests.rs"]
pub(super) mod tests;
