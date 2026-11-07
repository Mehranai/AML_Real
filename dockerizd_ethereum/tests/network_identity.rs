use ethereum_aml::domain::{AddressId, NetworkId};
use std::str::FromStr;

#[test]
fn wallet_identity_is_network_scoped() {
    let address = "0x0000000000000000000000000000000000000001";
    let ethereum = AddressId::parse_evm(NetworkId::ethereum_mainnet(), address).unwrap();
    let another_evm =
        AddressId::parse_evm(NetworkId::from_str("eip155:56").unwrap(), address).unwrap();

    assert_ne!(ethereum.canonical_key(), another_evm.canonical_key());
}
