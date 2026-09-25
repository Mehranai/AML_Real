use std::collections::HashSet;

use crate::services::tron::aml::flow_engine::compute_net_flows;
use crate::services::tron::aml::types::{AmlEvent, SimpleTransfer, ZERO_ADDRESS};

pub fn detect_liquidity_events(transfers: &[SimpleTransfer], actor: Option<&str>) -> Vec<AmlEvent> {
    let flows = compute_net_flows(transfers);
    let mut events = Vec::new();
    let mut dedup = HashSet::new();

    for (address, token_map) in flows {
        if actor.is_some_and(|actor| actor != address) {
            continue;
        }

        if address == ZERO_ADDRESS {
            continue;
        }

        let sent_tokens = token_map
            .iter()
            .filter(|(_, delta)| delta.sign() == num_bigint::Sign::Minus)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        let received_tokens = token_map
            .iter()
            .filter(|(_, delta)| delta.sign() == num_bigint::Sign::Plus)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();

        if sent_tokens.len() >= 2 && received_tokens.len() == 1 {
            let lp_token = received_tokens[0].clone();
            if !transfers
                .iter()
                .any(|t| t.token == lp_token && t.from == ZERO_ADDRESS && t.to == address)
            {
                continue;
            }
            let key = format!("add:{}:{}:{}", address, lp_token, sent_tokens.join(","));

            if dedup.insert(key) {
                events.push(AmlEvent::LiquidityAdd {
                    user: address,
                    lp_token,
                    sent_tokens,
                });
            }

            continue;
        }

        if sent_tokens.len() == 1 && received_tokens.len() >= 2 {
            let lp_token = sent_tokens[0].clone();
            // V2 LP tokens may be transferred to the pool and burned there.
            if !transfers
                .iter()
                .any(|t| t.token == lp_token && t.to == ZERO_ADDRESS)
            {
                continue;
            }
            let key = format!(
                "remove:{}:{}:{}",
                address,
                lp_token,
                received_tokens.join(",")
            );

            if dedup.insert(key) {
                events.push(AmlEvent::LiquidityRemove {
                    user: address,
                    lp_token,
                    received_tokens,
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(token: &str, from: &str, to: &str, amount: u128) -> SimpleTransfer {
        SimpleTransfer {
            token: token.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            raw_amount: amount.into(),
        }
    }

    #[test]
    fn detects_liquidity_add() {
        let transfers = vec![
            transfer("TRX", "wallet", "pool", 100),
            transfer("USDT", "wallet", "pool", 50),
            transfer("LP", ZERO_ADDRESS, "wallet", 10),
        ];

        let events = detect_liquidity_events(&transfers, Some("wallet"));

        assert!(matches!(events[0], AmlEvent::LiquidityAdd { .. }));
    }

    #[test]
    fn detects_liquidity_remove() {
        let transfers = vec![
            transfer("LP", "wallet", "pool", 10),
            transfer("LP", "pool", ZERO_ADDRESS, 10),
            transfer("TRX", "pool", "wallet", 100),
            transfer("USDT", "pool", "wallet", 50),
        ];

        let events = detect_liquidity_events(&transfers, Some("wallet"));

        assert!(matches!(events[0], AmlEvent::LiquidityRemove { .. }));
    }

    #[test]
    fn multi_asset_payment_is_not_liquidity_without_lp_evidence() {
        let transfers = vec![
            transfer("a", "wallet", "other", 10),
            transfer("b", "wallet", "other", 20),
            transfer("c", "other", "wallet", 5),
        ];
        assert!(detect_liquidity_events(&transfers, Some("wallet")).is_empty());
    }
}
