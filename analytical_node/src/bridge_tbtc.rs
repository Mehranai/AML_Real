use super::{finish, get_json, validate_observation, validate_occurrence, validate_snapshot};
use crate::{App, now};
use aml_chain_api::{
    Network,
    bridge::{
        Log, Observation, WINDOW_MS,
        tbtc::{
            CLOCK_SKEW_MS, EMITTER, FundingObservation, OUTPUT_LIMIT, Output, PROOF_DELAY_MS,
            REVEAL_TOPIC, SPEND_BLOCKS, SWEEP_TOPIC, Spend, SpendObservation, reverse_hash,
        },
    },
    kyt::transaction_hash,
};
use anyhow::{Context, ensure};
use bitcoin::ScriptBuf;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[path = "bridge_tbtc_mint.rs"]
mod mint;

const POLICY: &str = "tbtc_deposit_reveal_v1";
const SWEEP_POLICY: &str = "tbtc_deposit_sweep_v1";
const MAX_MONEY_SATS: u64 = 21_000_000 * 100_000_000;

pub(super) enum Stage {
    Reveal,
    Sweep,
    Mint,
}

pub(super) async fn verify(
    app: &App,
    hash: &str,
    block: u64,
    index: Option<u32>,
    stage: Stage,
) -> anyhow::Result<Result<Value, &'static str>> {
    let bitcoin = Network::Bitcoin;
    let ethereum = Network::Ethereum;
    let mut path = format!("/internal/bridge/tbtc/funding/{hash}?block_number={block}");
    if let Some(index) = index {
        path.push_str(&format!("&output_index={index}"));
    }
    let anchor: FundingObservation = get_json(app, bitcoin, &path, None).await?;
    validate_snapshot(bitcoin, &anchor.network_id, &anchor.source_snapshot)?;
    ensure!(
        anchor.outputs.len() <= OUTPUT_LIMIT,
        "Bitcoin output limit exceeded"
    );
    if anchor.truncated {
        return Ok(Err("ambiguous_anchor"));
    }
    let Some(first) = anchor.outputs.first() else {
        return Ok(Err("anchor_not_found"));
    };
    let mut indices = BTreeSet::new();
    for output in &anchor.outputs {
        validate_occurrence(
            &anchor.source_snapshot,
            output.block_number,
            &output.block_hash,
            output.timestamp_ms,
        )?;
        ensure!(
            output.tx_hash == hash
                && output.block_number == block
                && transaction_hash(bitcoin, &output.block_hash)? == output.block_hash
                && output.block_hash == first.block_hash
                && output.timestamp_ms == first.timestamp_ms
                && index.is_none_or(|wanted| output.output_index == wanted)
                && output.value_sats <= MAX_MONEY_SATS
                && indices.insert(output.output_index),
            "invalid Bitcoin funding evidence"
        );
    }
    // The deposit can be revealed before the Bitcoin funding transaction is mined.
    let from = first.timestamp_ms.saturating_sub(WINDOW_MS);
    let to = first.timestamp_ms.saturating_add(WINDOW_MS);
    let mut path = format!("/internal/bridge/tbtc/reveals/{hash}?from_ms={from}&to_ms={to}");
    if let Some(index) = index {
        path.push_str(&format!("&output_index={index}"));
    }
    let peer: Observation = get_json(app, ethereum, &path, None).await?;
    validate_observation(ethereum, &peer)?;
    if peer.truncated {
        return Ok(Err(if index.is_none() {
            "ambiguous_anchor"
        } else {
            "ambiguous_peer"
        }));
    }
    if peer.logs.is_empty() {
        return Ok(Err("peer_not_found_in_indexed_window"));
    }
    let mut matched: Option<(&Output, &Log, Reveal)> = None;
    for log in &peer.logs {
        let reveal = Reveal::decode(log).context("invalid tBTC reveal log")?;
        ensure!(
            reveal.funding_txid == hash
                && index.is_none_or(|wanted| reveal.output_index == wanted)
                && (from..=to).contains(&log.timestamp_ms),
            "unexpected tBTC reveal log"
        );
        if let Some(output) = anchor.outputs.iter().find(|output| reveal.matches(output)) {
            if let Some((prior, _, _)) = &matched {
                return Ok(Err(if prior.output_index == output.output_index {
                    "ambiguous_peer"
                } else {
                    "ambiguous_anchor"
                }));
            }
            matched = Some((output, log, reveal));
        }
    }
    let Some((output, log, reveal)) = matched else {
        return Ok(Err("inconsistent_pair"));
    };
    let deposit = json!({
        "protocol": "threshold_tbtc_v2", "policy": POLICY, "claim": "deposit_reveal",
        "operation_id": format!("{hash}:{}", output.output_index), "asset": "BTC",
        "confidence": "high", "validity": "historical_capture", "captured_at_unix_ms": now(),
        "settlement_verified": false, "mint_verified": false, "fund_continuity_claimed": false,
        "send": {
            "event": "bitcoin_funding_output", "network_id": bitcoin.id(),
            "receipt": output, "source_snapshot": anchor.source_snapshot,
            "amount_raw": output.value_sats.to_string(), "decimals": 8
        },
        "relay": {
            "event": "deposit_revealed", "network_id": ethereum.id(),
            "receipt": log, "source_snapshot": peer.source_snapshot,
            "depositor": reveal.depositor, "vault": reveal.vault
        }
    });
    let verified = match stage {
        Stage::Reveal => Ok(deposit),
        Stage::Sweep => sweep(app, output, log, &reveal.wallet, deposit).await?,
        Stage::Mint => mint::verify(app, output, log, &reveal, deposit).await?,
    };
    let pair = match verified {
        Ok(pair) => pair,
        Err(status) => return Ok(Err(status)),
    };
    finish(
        app,
        (bitcoin, &anchor.source_snapshot),
        (ethereum, &peer.source_snapshot),
        pair,
    )
    .await
}

async fn sweep(
    app: &App,
    output: &Output,
    reveal: &Log,
    wallet: &str,
    deposit: Value,
) -> anyhow::Result<Result<Value, &'static str>> {
    let bitcoin = Network::Bitcoin;
    let ethereum = Network::Ethereum;
    let path = format!(
        "/internal/bridge/tbtc/spends/{}?block_number={}&output_index={}",
        output.tx_hash, output.block_number, output.output_index,
    );
    let source: SpendObservation = get_json(app, bitcoin, &path, None).await?;
    validate_snapshot(bitcoin, &source.network_id, &source.source_snapshot)?;
    if source.source_snapshot != deposit["send"]["source_snapshot"] {
        return Ok(Err("source_changed"));
    }
    let spend = match source.inputs.as_slice() {
        [] => return Ok(Err("sweep_spend_not_found")),
        [spend] => spend,
        _ => return Ok(Err("ambiguous_sweep_spend")),
    };
    validate_occurrence(
        &source.source_snapshot,
        spend.block_number,
        &spend.block_hash,
        spend.timestamp_ms,
    )?;
    ensure!(
        spends_deposit(spend, output)
            && transaction_hash(bitcoin, &spend.tx_hash)? == spend.tx_hash
            && transaction_hash(bitcoin, &spend.block_hash)? == spend.block_hash,
        "invalid tBTC spending input"
    );
    let (from, to) = proof_window(spend, reveal);
    if from > to {
        return Ok(Err("sweep_proof_not_found"));
    }
    let path = format!(
        "/internal/bridge/tbtc/sweeps/{}?from_ms={from}&to_ms={to}",
        spend.tx_hash
    );
    let proof: Observation = get_json(app, ethereum, &path, None).await?;
    validate_observation(ethereum, &proof)?;
    if proof.source_snapshot != deposit["relay"]["source_snapshot"] {
        return Ok(Err("source_changed"));
    }
    if proof.truncated || proof.logs.len() > 1 {
        return Ok(Err("ambiguous_sweep_proof"));
    }
    let Some(log) = proof.logs.first() else {
        return Ok(Err("sweep_proof_not_found"));
    };
    if !confirms_sweep(log, spend, reveal, wallet) {
        return Ok(Err("inconsistent_sweep"));
    }
    Ok(Ok(json!({
        "protocol": "threshold_tbtc_v2", "policy": SWEEP_POLICY, "claim": "deposit_sweep",
        "operation_id": deposit["operation_id"], "asset": "BTC",
        "confidence": "high", "validity": "historical_capture", "captured_at_unix_ms": now(),
        "sweep_verified": true, "settlement_verified": false,
        "mint_verified": false, "fund_continuity_claimed": false,
        "deposit": {"funding": deposit["send"], "reveal": deposit["relay"]},
        "send": {
            "event": "bitcoin_spend_input", "network_id": bitcoin.id(),
            "receipt": spend, "source_snapshot": source.source_snapshot,
            "amount_raw": output.value_sats.to_string(), "decimals": 8
        },
        "relay": {
            "event": "deposits_swept", "network_id": ethereum.id(),
            "receipt": log, "source_snapshot": proof.source_snapshot,
            "wallet_pubkey_hash": format!("0x{wallet}")
        }
    })))
}

fn spends_deposit(spend: &Spend, output: &Output) -> bool {
    spend.prev_tx_hash == output.tx_hash
        && spend.prev_output_index == output.output_index
        && spend.tx_hash != output.tx_hash
        && (output.block_number..=output.block_number.saturating_add(SPEND_BLOCKS))
            .contains(&spend.block_number)
}

fn proof_window(spend: &Spend, reveal: &Log) -> (u64, u64) {
    (
        reveal
            .timestamp_ms
            .max(spend.timestamp_ms.saturating_sub(CLOCK_SKEW_MS)),
        spend.timestamp_ms.saturating_add(PROOF_DELAY_MS),
    )
}

fn confirms_sweep(log: &Log, spend: &Spend, reveal: &Log, wallet: &str) -> bool {
    let [topic] = log.topics.as_slice() else {
        return false;
    };
    let data = digits(&log.data);
    let ethereum = Network::Ethereum;
    let (from, to) = proof_window(spend, reveal);
    log.address == EMITTER
        && digits(topic) == SWEEP_TOPIC
        && is_hex(data, 128)
        && fixed_bytes(&data[..64], 20).is_some_and(|word| word.eq_ignore_ascii_case(wallet))
        && reverse_hash(&data[64..]).as_deref() == Some(spend.tx_hash.as_str())
        && (log.block_number, log.log_index) > (reveal.block_number, reveal.log_index)
        && (from..=to).contains(&log.timestamp_ms)
        && transaction_hash(ethereum, &log.tx_hash).is_ok_and(|hash| hash == log.tx_hash)
        && transaction_hash(ethereum, &log.block_hash).is_ok_and(|hash| hash == log.block_hash)
}

struct Reveal {
    funding_txid: String,
    output_index: u32,
    value_sats: u64,
    depositor: String,
    vault: String,
    wallet: String,
    script: ScriptBuf,
}

impl Reveal {
    fn decode(log: &Log) -> Option<Self> {
        if log.address != EMITTER
            || transaction_hash(Network::Ethereum, &log.tx_hash).ok()? != log.tx_hash
            || transaction_hash(Network::Ethereum, &log.block_hash).ok()? != log.block_hash
        {
            return None;
        }
        let [topic, depositor, wallet] = log.topics.as_slice() else {
            return None;
        };
        let data = digits(&log.data);
        if digits(topic) != REVEAL_TOPIC || !is_hex(data, 7 * 64) {
            return None;
        }
        let word = |i: usize| &data[i * 64..(i + 1) * 64];
        let depositor = address(digits(depositor))?;
        let wallet = fixed_bytes(digits(wallet), 20)?;
        let blinding = fixed_bytes(word(3), 8)?;
        let refund = fixed_bytes(word(4), 20)?;
        let locktime = fixed_bytes(word(5), 4)?;
        let value_sats = u64::from_str_radix(word(2), 16).ok()?;
        if value_sats == 0 || value_sats > MAX_MONEY_SATS || depositor.bytes().all(|b| b == b'0') {
            return None;
        }
        // Standard tBTC deposit script. Extra-data reveals cannot reproduce this commitment.
        let script = ScriptBuf::from_hex(&format!(
            "14{depositor}7508{blinding}7576a914{wallet}8763ac6776a914{refund}8804{locktime}b175ac68"
        )).ok()?;
        Some(Self {
            funding_txid: reverse_hash(word(0))?,
            output_index: u32::from_str_radix(word(1), 16).ok()?,
            value_sats,
            depositor: format!("0x{}", depositor.to_ascii_lowercase()),
            vault: format!("0x{}", address(word(6))?.to_ascii_lowercase()),
            wallet: wallet.to_ascii_lowercase(),
            script,
        })
    }

    fn matches(&self, output: &Output) -> bool {
        self.funding_txid == output.tx_hash
            && self.output_index == output.output_index
            && self.value_sats == output.value_sats
            && ScriptBuf::from_hex(&output.script_hex).is_ok_and(|script| {
                script == self.script.to_p2wsh() || script == self.script.to_p2sh()
            })
    }
}

fn digits(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn fixed_bytes(word: &str, bytes: usize) -> Option<&str> {
    if !is_hex(word, 64) {
        return None;
    }
    let (value, pad) = word.split_at(bytes * 2);
    pad.bytes().all(|b| b == b'0').then_some(value)
}

fn address(word: &str) -> Option<&str> {
    if !is_hex(word, 64) {
        return None;
    }
    let (pad, value) = word.split_at(24);
    pad.bytes().all(|b| b == b'0').then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha3::{Digest, Keccak256};

    #[test]
    fn mainnet_tbtc_reveal_requires_exact_funding_evidence() {
        let topic = Keccak256::digest(
            b"DepositRevealed(bytes32,uint32,address,uint64,bytes8,bytes20,bytes20,bytes4,address)",
        );
        let topic = topic
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(topic, REVEAL_TOPIC);
        let fixture: Value = serde_json::from_str(include_str!(
            "../../shared/chain_api/tests/fixtures/tbtc-mainnet.json"
        ))
        .unwrap();
        let mut log: Log = serde_json::from_value(fixture["eth"].clone()).unwrap();
        let mut output: Output = serde_json::from_value(fixture["btc"].clone()).unwrap();
        let reveal = Reveal::decode(&log).unwrap();
        assert!(reveal.matches(&output));
        assert!(log.timestamp_ms < output.timestamp_ms);
        output.value_sats += 1;
        assert!(!reveal.matches(&output));
        output.value_sats -= 1;
        output.output_index += 1;
        assert!(!reveal.matches(&output));
        output.output_index -= 1;
        output.tx_hash = reverse_hash(&output.tx_hash).unwrap();
        assert!(!reveal.matches(&output));
        output.tx_hash = reveal.funding_txid.clone();
        output.script_hex.replace_range(4..5, "0");
        assert!(!reveal.matches(&output));
        output.script_hex = format!("{:x}", reveal.script.to_p2sh());
        assert!(reveal.matches(&output));
        log.topics[2].replace_range(65..66, "1");
        assert!(Reveal::decode(&log).is_none());
    }

    #[test]
    fn mainnet_tbtc_sweep_requires_exact_spent_outpoint_and_bridge_proof() {
        let topic = Keccak256::digest(b"DepositsSwept(bytes20,bytes32)");
        let topic = topic
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(topic, SWEEP_TOPIC);
        let fixture: Value = serde_json::from_str(include_str!(
            "../../shared/chain_api/tests/fixtures/tbtc-sweep-mainnet.json"
        ))
        .unwrap();
        let output: Output = serde_json::from_value(fixture["btc"].clone()).unwrap();
        let reveal: Log = serde_json::from_value(fixture["eth"].clone()).unwrap();
        let deposit = Reveal::decode(&reveal).unwrap();
        let mut spend: Spend = serde_json::from_value(fixture["spend"].clone()).unwrap();
        let proof: Log = serde_json::from_value(fixture["proof"].clone()).unwrap();
        assert!(deposit.matches(&output));
        assert!(spends_deposit(&spend, &output));
        assert!(confirms_sweep(&proof, &spend, &reveal, &deposit.wallet));
        spend.prev_output_index += 1;
        assert!(!spends_deposit(&spend, &output));
        spend.prev_output_index -= 1;
        spend.block_number = output.block_number + SPEND_BLOCKS + 1;
        assert!(!spends_deposit(&spend, &output));
        spend.block_number = output.block_number;
        spend.prev_tx_hash = "1".repeat(64);
        assert!(!spends_deposit(&spend, &output));
        for (range, value) in [(2..3, "a"), (65..66, "1"), (66..67, "f")] {
            let mut altered = proof.clone();
            altered.data.replace_range(range, value);
            assert!(!confirms_sweep(&altered, &spend, &reveal, &deposit.wallet));
        }
        let mut altered = proof.clone();
        altered.block_number = reveal.block_number;
        altered.log_index = reveal.log_index;
        assert!(!confirms_sweep(&altered, &spend, &reveal, &deposit.wallet));
        altered = proof.clone();
        altered.timestamp_ms = spend.timestamp_ms + PROOF_DELAY_MS + 1;
        assert!(!confirms_sweep(&altered, &spend, &reveal, &deposit.wallet));
        altered = proof.clone();
        altered.address = format!("0x{}", "1".repeat(40));
        assert!(!confirms_sweep(&altered, &spend, &reveal, &deposit.wallet));
    }
}
