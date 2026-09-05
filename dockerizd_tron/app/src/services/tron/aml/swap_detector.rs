use crate::services::tron::aml::flow_engine::compute_net_flows;
use crate::services::tron::aml::types::{AmlEvent, SimpleTransfer, ZERO_ADDRESS};
use std::collections::HashSet;

pub fn detect_swaps(transfers: &[SimpleTransfer], actor: Option<&str>) -> Vec<AmlEvent> {
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

        let mut sent = Vec::new();
        let mut received = Vec::new();

        for (token, delta) in token_map {
            if delta < 0 {
                sent.push(token.clone());
            }

            if delta > 0 {
                received.push(token.clone());
            }
        }

        if sent.is_empty() || received.is_empty() {
            continue;
        }

        for token_in in &sent {
            for token_out in &received {
                if token_in == token_out {
                    continue;
                }

                let key = format!("{}:{}:{}", address, token_in, token_out);

                if dedup.contains(&key) {
                    continue;
                }

                dedup.insert(key);

                events.push(AmlEvent::Swap {
                    user: address.clone(),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                });
            }
        }
    }

    events
}
