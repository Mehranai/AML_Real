use crate::services::tron::aml::types::{AmlEvent, SimpleTransfer, ZERO_ADDRESS};

pub fn detect_mints_and_burns(transfers: &[SimpleTransfer]) -> Vec<AmlEvent> {
    let mut events = Vec::new();

    for t in transfers {
        //
        // mint
        //
        if t.from == ZERO_ADDRESS {
            events.push(AmlEvent::Mint {
                user: t.to.clone(),
                token: t.token.clone(),
            });
        }

        //
        // burn
        //
        if t.to == ZERO_ADDRESS {
            events.push(AmlEvent::Burn {
                user: t.from.clone(),
                token: t.token.clone(),
            });
        }
    }

    events
}
