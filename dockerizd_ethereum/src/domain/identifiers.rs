use std::{fmt, str::FromStr};

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkId(String);

impl NetworkId {
    pub fn ethereum_mainnet() -> Self {
        Self("eip155:1".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for NetworkId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        let Some((namespace, reference)) = normalized.split_once(':') else {
            return Err(IdentifierError::InvalidNetwork(value.to_string()));
        };

        if namespace.is_empty()
            || reference.is_empty()
            || !namespace
                .chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
            || !reference.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '_')
            })
        {
            return Err(IdentifierError::InvalidNetwork(value.to_string()));
        }

        Ok(Self(normalized))
    }
}

impl fmt::Display for NetworkId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AddressId {
    network: NetworkId,
    address: String,
}

impl AddressId {
    pub fn parse_evm(network: NetworkId, value: &str) -> Result<Self, IdentifierError> {
        let address = Address::from_str(value)
            .map_err(|_| IdentifierError::InvalidEvmAddress(value.to_string()))?;

        Ok(Self {
            network,
            address: format!("{address:#x}"),
        })
    }

    pub fn network(&self) -> &NetworkId {
        &self.network
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn canonical_key(&self) -> String {
        format!("{}:{}", self.network, self.address)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId {
    network: NetworkId,
    asset: String,
}

impl AssetId {
    pub fn native_eth(network: NetworkId) -> Self {
        Self {
            network,
            asset: "native:eth".to_string(),
        }
    }

    pub fn erc20(network: NetworkId, token_address: &str) -> Result<Self, IdentifierError> {
        Self::evm_token(network, "erc20", token_address)
    }

    pub fn erc721(network: NetworkId, token_address: &str) -> Result<Self, IdentifierError> {
        Self::evm_token(network, "erc721", token_address)
    }

    pub fn erc1155(network: NetworkId, token_address: &str) -> Result<Self, IdentifierError> {
        Self::evm_token(network, "erc1155", token_address)
    }

    fn evm_token(
        network: NetworkId,
        standard: &str,
        token_address: &str,
    ) -> Result<Self, IdentifierError> {
        let address = Address::from_str(token_address)
            .map_err(|_| IdentifierError::InvalidEvmAddress(token_address.to_string()))?;

        Ok(Self {
            network,
            asset: format!("{standard}:{address:#x}"),
        })
    }

    pub fn network(&self) -> &NetworkId {
        &self.network
    }

    pub fn canonical_key(&self) -> String {
        format!("{}/{}", self.network, self.asset)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdentifierError {
    #[error("invalid network identifier: {0}")]
    InvalidNetwork(String),
    #[error("invalid EVM address: {0}")]
    InvalidEvmAddress(String),
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{AddressId, AssetId, NetworkId};

    #[test]
    fn network_qualifies_same_evm_address() {
        let value = "0x00000000000000000000000000000000000000AA";
        let ethereum = AddressId::parse_evm(NetworkId::ethereum_mainnet(), value).unwrap();
        let polygon =
            AddressId::parse_evm(NetworkId::from_str("eip155:137").unwrap(), value).unwrap();

        assert_ne!(ethereum.canonical_key(), polygon.canonical_key());
        assert_eq!(
            ethereum.address(),
            "0x00000000000000000000000000000000000000aa"
        );
    }

    #[test]
    fn assets_are_network_qualified() {
        let asset = AssetId::erc20(
            NetworkId::ethereum_mainnet(),
            "0x00000000000000000000000000000000000000AA",
        )
        .unwrap();

        assert_eq!(
            asset.canonical_key(),
            "eip155:1/erc20:0x00000000000000000000000000000000000000aa"
        );
    }
}
