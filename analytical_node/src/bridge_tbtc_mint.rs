use super::{
    App, Log, Network, Output, Reveal, address, digits, get_json, is_hex, json, now,
    transaction_hash, validate_observation,
};
use aml_chain_api::bridge::tbtc::{
    MINT_FINALIZED_TOPIC, MINT_WINDOW_MS, MINTED_TOPIC, MintObservation, TOKEN, TRANSFER_TOPIC,
    VAULT,
};
use anyhow::Context;
use bitcoin::{Txid, hashes::Hash};
use serde_json::Value;
use sha3::{Digest, Keccak256};
use std::collections::BTreeSet;

const POLICY: &str = "tbtc_optimistic_mint_v1";
const UNITS_PER_SAT: u128 = 10_000_000_000;

pub(super) async fn verify(
    app: &App,
    output: &Output,
    reveal_log: &Log,
    reveal: &Reveal,
    deposit: Value,
) -> anyhow::Result<Result<Value, &'static str>> {
    if reveal.vault != VAULT {
        return Ok(Err("unsupported_mint_vault"));
    }
    let key = deposit_key(output).context("invalid tBTC funding transaction hash")?;
    let from = reveal_log.timestamp_ms;
    let to = from.saturating_add(MINT_WINDOW_MS);
    let path = format!("/internal/bridge/tbtc/mints/{key}?from_ms={from}&to_ms={to}");
    let ethereum = Network::Ethereum;
    let mint: MintObservation = get_json(app, ethereum, &path, None).await?;
    let proof = &mint.finalizations;
    validate_observation(ethereum, proof)?;
    if proof.source_snapshot != deposit["relay"]["source_snapshot"] {
        return Ok(Err("source_changed"));
    }
    if proof.truncated || proof.logs.len() > 1 {
        return Ok(Err("ambiguous_mint"));
    }
    let Some(finalization) = proof.logs.first() else {
        return Ok(Err("mint_not_found"));
    };
    let Some((issuance, fee)) = issued_tokens(
        output,
        reveal_log,
        reveal,
        &key,
        finalization,
        &mint.preceding_logs,
    ) else {
        return Ok(Err("inconsistent_mint"));
    };
    Ok(Ok(json!({
        "protocol": "threshold_tbtc_v2", "policy": POLICY, "claim": "optimistic_mint",
        "operation_id": deposit["operation_id"], "asset": "tBTC",
        "confidence": "high", "validity": "historical_capture", "captured_at_unix_ms": now(),
        "mint_verified": true, "settlement_verified": false, "fund_continuity_claimed": false,
        "send": deposit["send"],
        "deposit_reveal": deposit["relay"],
        "relay": {
            "event": "optimistic_minting_finalized", "network_id": ethereum.id(),
            "receipt": finalization, "source_snapshot": proof.source_snapshot, "deposit_key": key,
            "recipient": reveal.depositor, "token_address": TOKEN, "decimals": 18,
            "amount_raw": issuance.amount.to_string(), "vault_mint": issuance.minted,
            "token_transfer": issuance.transfer,
            "fee_mint": fee.map(|fee| json!({
                "recipient":format!("0x{}",fee.recipient),
                "amount_raw":fee.amount.to_string(),"vault_mint":fee.minted,"token_transfer":fee.transfer
            }))
        }
    })))
}

fn deposit_key(output: &Output) -> Option<String> {
    let hash: Txid = output.tx_hash.parse().ok()?;
    let mut keccak = Keccak256::new();
    keccak.update(hash.to_byte_array());
    keccak.update(output.output_index.to_be_bytes());
    let digest = keccak.finalize();
    let key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("0x{key}"))
}

fn issued_tokens<'a>(
    output: &Output,
    reveal_log: &Log,
    reveal: &Reveal,
    key: &str,
    finalization: &Log,
    preceding: &'a [Log],
) -> Option<(MintPair<'a>, Option<MintPair<'a>>)> {
    let [topic, minter, deposit, recipient] = finalization.topics.as_slice() else {
        return None;
    };
    let depositor = digits(&reveal.depositor);
    let eth = Network::Ethereum;
    if finalization.address != VAULT
        || reveal.vault != VAULT
        || digits(topic) != MINT_FINALIZED_TOPIC
        || !nonzero_address(minter)
        || digits(deposit) != digits(key)
        || address(digits(recipient))? != depositor
        || (finalization.block_number, finalization.log_index)
            <= (reveal_log.block_number, reveal_log.log_index)
        || !(reveal_log.timestamp_ms..=reveal_log.timestamp_ms.saturating_add(MINT_WINDOW_MS))
            .contains(&finalization.timestamp_ms)
        || transaction_hash(eth, &finalization.tx_hash).ok()? != finalization.tx_hash
        || transaction_hash(eth, &finalization.block_hash).ok()? != finalization.block_hash
        || preceding.len() > 4
    {
        return None;
    }
    let mut indices = BTreeSet::new();
    let index = finalization.log_index;
    for log in preceding {
        if log.tx_hash != finalization.tx_hash
            || log.block_hash != finalization.block_hash
            || log.block_number != finalization.block_number
            || log.timestamp_ms != finalization.timestamp_ms
            || !(index.saturating_sub(4)..index).contains(&log.log_index)
            || !indices.insert(log.log_index)
        {
            return None;
        }
    }
    let at = |offset| {
        let target = index.checked_sub(offset)?;
        preceding.iter().find(|log| log.log_index == target)
    };
    let last = MintPair::decode(at(2)?, at(1)?)?;
    let earlier = at(4)
        .zip(at(3))
        .and_then(|(minted, transfer)| MintPair::decode(minted, transfer));
    // The vault mints to the depositor, optionally mints the fee, then finalizes.
    let (issuance, fee) = if last.recipient == depositor {
        if earlier.is_some_and(|pair| pair.recipient == depositor) {
            return None;
        }
        (last, None)
    } else {
        (
            earlier.filter(|pair| pair.recipient == depositor)?,
            Some(last),
        )
    };
    let minted_total = issuance
        .amount
        .checked_add(fee.as_ref().map_or(0, |pair| pair.amount))?;
    let funding = u128::from(output.value_sats) * UNITS_PER_SAT;
    let debt = amount(finalization)?;
    (minted_total <= funding && minted_total <= debt).then_some((issuance, fee))
}

struct MintPair<'a> {
    minted: &'a Log,
    transfer: &'a Log,
    recipient: &'a str,
    amount: u128,
}

impl<'a> MintPair<'a> {
    fn decode(minted: &'a Log, transfer: &'a Log) -> Option<Self> {
        let [mint_topic, recipient] = minted.topics.as_slice() else {
            return None;
        };
        let [transfer_topic, from, to] = transfer.topics.as_slice() else {
            return None;
        };
        let value = amount(minted)?;
        if minted.address != VAULT
            || transfer.address != TOKEN
            || digits(mint_topic) != MINTED_TOPIC
            || digits(transfer_topic) != TRANSFER_TOPIC
            || address(digits(from))? != "0000000000000000000000000000000000000000"
            || !nonzero_address(recipient)
            || address(digits(to))? != address(digits(recipient))?
            || value == 0
            || amount(transfer)? != value
        {
            return None;
        }
        Some(Self {
            minted,
            transfer,
            recipient: address(digits(recipient))?,
            amount: value,
        })
    }
}

fn amount(log: &Log) -> Option<u128> {
    let data = digits(&log.data);
    if !is_hex(data, 64) {
        return None;
    }
    u128::from_str_radix(data, 16).ok()
}

fn nonzero_address(topic: &str) -> bool {
    address(digits(topic)).is_some_and(|address| address.bytes().any(|byte| byte != b'0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_tbtc_mint_binds_outpoint_receipt_and_token_amount() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../shared/chain_api/tests/fixtures/tbtc-mint-mainnet.json"
        ))
        .unwrap();
        let mut output: Output = serde_json::from_value(fixture["btc"].clone()).unwrap();
        let reveal_log: Log = serde_json::from_value(fixture["eth"].clone()).unwrap();
        let reveal = Reveal::decode(&reveal_log).unwrap();
        assert!(reveal.matches(&output));
        let [minted, transfer, finalized]: [Log; 3] =
            serde_json::from_value(fixture["mint"].clone()).unwrap();
        let key = deposit_key(&output).unwrap();
        assert_eq!(key, finalized.topics[2]);
        output.output_index = 1;
        assert_ne!(deposit_key(&output).unwrap(), key);
        output.output_index = 0;
        let check = |event: &Log, logs: &[Log]| {
            issued_tokens(&output, &reveal_log, &reveal, &key, event, logs)
                .map(|(issued, fee)| (issued.amount, fee.map(|fee| fee.amount)))
        };
        let mut logs = vec![minted, transfer];
        let issued = 10_500_000_000_000_000u128;
        assert_eq!(check(&finalized, &logs), Some((issued, None)));
        assert_eq!(check(&finalized, &logs[..1]), None);
        let mut bad = finalized.clone();
        bad.topics[2] = format!("0x{}", "a".repeat(64));
        assert_eq!(check(&bad, &logs), None);
        bad = finalized.clone();
        bad.timestamp_ms = reveal_log.timestamp_ms + MINT_WINDOW_MS + 1;
        assert_eq!(check(&bad, &logs), None);
        let mut bad_logs = logs.clone();
        bad_logs[1].address = VAULT.into();
        assert_eq!(check(&finalized, &bad_logs), None);
        bad_logs[1] = logs[1].clone();
        bad_logs[1].topics[1] = bad_logs[1].topics[2].clone();
        assert_eq!(check(&finalized, &bad_logs), None);
        bad_logs[1] = logs[1].clone();
        bad_logs[1].data = format!("0x{:064x}", issued - 1);
        assert_eq!(check(&finalized, &bad_logs), None);
        bad_logs[1] = logs[1].clone();
        bad_logs[1].tx_hash = format!("0x{}", "a".repeat(64));
        assert_eq!(check(&finalized, &bad_logs), None);

        let mut with_fee = finalized.clone();
        with_fee.log_index += 2;
        let fee_recipient = format!("0x{:0>64}", "1".repeat(40));
        let fee = 1_000_000u128;
        logs[0].data = format!("0x{:064x}", issued - fee);
        logs[1].data = logs[0].data.clone();
        let mut fee_mint = logs[0].clone();
        fee_mint.log_index += 2;
        fee_mint.topics[1] = fee_recipient.clone();
        fee_mint.data = format!("0x{fee:064x}");
        let mut fee_transfer = logs[1].clone();
        fee_transfer.log_index += 2;
        fee_transfer.topics[2] = fee_recipient;
        fee_transfer.data = fee_mint.data.clone();
        logs.extend([fee_mint, fee_transfer]);
        assert_eq!(check(&with_fee, &logs), Some((issued - fee, Some(fee))));
        logs[2].topics[1] = logs[0].topics[1].clone();
        logs[3].topics[2] = logs[1].topics[2].clone();
        assert_eq!(check(&with_fee, &logs), None);
    }
}
