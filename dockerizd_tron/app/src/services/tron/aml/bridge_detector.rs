use crate::services::tron::aml::flow_engine::compute_net_flows;

use crate::services::tron::aml::types::{AmlEvent, SimpleTransfer, ZERO_ADDRESS};

pub fn detect_bridges(transfers: &[SimpleTransfer], bridge_protocol_hint: bool) -> Vec<AmlEvent> {
    if !bridge_protocol_hint {
        return Vec::new();
    }

    let flows = compute_net_flows(transfers);

    let mut events = Vec::new();

    for (address, token_map) in flows {
        if address == ZERO_ADDRESS {
            continue;
        }

        for (token, delta) in token_map {
            //
            // bridge in
            //
            if delta > 0
                && transfers
                    .iter()
                    .any(|t| t.from == ZERO_ADDRESS && t.to == address && t.token == token)
            {
                events.push(AmlEvent::BridgeIn {
                    user: address.clone(),
                    token: token.clone(),
                });
            }

            //
            // bridge out
            //
            if delta < 0
                && transfers
                    .iter()
                    .any(|t| t.to == ZERO_ADDRESS && t.from == address && t.token == token)
            {
                events.push(AmlEvent::BridgeOut {
                    user: address.clone(),
                    token: token.clone(),
                });
            }
        }
    }

    events
}
