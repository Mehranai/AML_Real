use crate::services::tron::aml::types::{FlowMap, SimpleTransfer};
use num_bigint::{BigInt, Sign};

pub fn compute_net_flows(transfers: &[SimpleTransfer]) -> FlowMap {
    let mut flows = FlowMap::new();

    for t in transfers {
        // A transaction may sum several UInt256 legs; even signed 256 bits is insufficient.
        let amount = BigInt::from_bytes_le(Sign::Plus, &t.raw_amount.to_le_bytes());
        *flows
            .entry(t.from.clone())
            .or_default()
            .entry(t.token.clone())
            .or_default() -= &amount;
        *flows
            .entry(t.to.clone())
            .or_default()
            .entry(t.token.clone())
            .or_default() += &amount;
    }

    flows
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickhouse::types::UInt256;

    #[test]
    fn full_width_transfers_and_aggregates_are_exact() {
        let leg = SimpleTransfer {
            token: "asset".into(),
            from: "a".into(),
            to: "b".into(),
            raw_amount: UInt256::from_le_bytes([255; 32]),
        };
        let flows = compute_net_flows(&[leg.clone(), leg.clone()]);
        let expected = BigInt::from_bytes_le(Sign::Plus, &[255; 32]) * 2;
        assert_eq!(flows["b"]["asset"], expected);
        assert_eq!(flows["a"]["asset"], -&expected);
        let reverse = SimpleTransfer {
            from: "b".into(),
            to: "a".into(),
            ..leg.clone()
        };
        assert_eq!(
            compute_net_flows(&[leg.clone(), reverse])["a"]["asset"],
            BigInt::from(0)
        );
        let self_transfer = SimpleTransfer {
            to: "a".into(),
            ..leg
        };
        assert_eq!(
            compute_net_flows(&[self_transfer])["a"]["asset"],
            BigInt::from(0)
        );
    }
}
