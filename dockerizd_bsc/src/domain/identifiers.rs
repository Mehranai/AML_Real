use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::BSC_NETWORK_ID;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkId(String);

impl NetworkId {
    pub fn bsc_mainnet() -> Self {
        Self(BSC_NETWORK_ID.to_string())
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
            return Err(IdentifierError::InvalidNetwork);
        };
        let valid_namespace = !namespace.is_empty()
            && namespace
                .chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit());
        let valid_reference = !reference.is_empty()
            && reference.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '_')
            });
        if !valid_namespace || !valid_reference {
            return Err(IdentifierError::InvalidNetwork);
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
        Ok(Self {
            network,
            address: normalize_evm_address(value)?,
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
    pub fn native_bnb() -> Self {
        Self {
            network: NetworkId::bsc_mainnet(),
            asset: "native:bnb".to_string(),
        }
    }

    pub fn erc20(network: NetworkId, contract: &str) -> Result<Self, IdentifierError> {
        Self::token(network, "erc20", contract, None)
    }

    pub fn erc721(
        network: NetworkId,
        contract: &str,
        token_id: &str,
    ) -> Result<Self, IdentifierError> {
        Self::token(network, "erc721", contract, Some(token_id))
    }

    pub fn erc1155(
        network: NetworkId,
        contract: &str,
        token_id: &str,
    ) -> Result<Self, IdentifierError> {
        Self::token(network, "erc1155", contract, Some(token_id))
    }

    fn token(
        network: NetworkId,
        standard: &str,
        contract: &str,
        token_id: Option<&str>,
    ) -> Result<Self, IdentifierError> {
        let contract = normalize_evm_address(contract)?;
        let asset = match token_id {
            Some(token_id) => {
                let token_id = normalize_token_id(token_id)?;
                format!("{standard}:{contract}/{token_id}")
            }
            None => format!("{standard}:{contract}"),
        };
        Ok(Self { network, asset })
    }

    pub fn canonical_key(&self) -> String {
        format!("{}/{}", self.network, self.asset)
    }
}

fn normalize_evm_address(value: &str) -> Result<String, IdentifierError> {
    let value = value.trim();
    let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    else {
        return Err(IdentifierError::InvalidEvmAddress);
    };
    if hex.len() != 40 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(IdentifierError::InvalidEvmAddress);
    }
    Ok(format!("0x{}", hex.to_ascii_lowercase()))
}

fn normalize_token_id(value: &str) -> Result<String, IdentifierError> {
    let value = value.trim();
    if value.is_empty() || !value.chars().all(|character| character.is_ascii_digit()) {
        return Err(IdentifierError::InvalidTokenId);
    }
    let normalized = value.trim_start_matches('0');
    Ok(if normalized.is_empty() {
        "0".to_string()
    } else {
        normalized.to_string()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentifierError {
    #[error("invalid network identifier")]
    InvalidNetwork,
    #[error("invalid EVM address")]
    InvalidEvmAddress,
    #[error("invalid decimal token id")]
    InvalidTokenId,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{AddressId, AssetId, IdentifierError, NetworkId};

    const ADDRESS: &str = "0x00000000000000000000000000000000000000AA";

    #[test]
    fn network_qualifies_the_same_evm_address() {
        let bsc = AddressId::parse_evm(NetworkId::bsc_mainnet(), ADDRESS).unwrap();
        let ethereum =
            AddressId::parse_evm(NetworkId::from_str("eip155:1").unwrap(), ADDRESS).unwrap();
        assert_ne!(bsc.canonical_key(), ethereum.canonical_key());
        assert_eq!(bsc.address(), "0x00000000000000000000000000000000000000aa");
    }

    #[test]
    fn creates_network_qualified_bsc_assets() {
        assert_eq!(
            AssetId::native_bnb().canonical_key(),
            "eip155:56/native:bnb"
        );
        assert_eq!(
            AssetId::erc20(NetworkId::bsc_mainnet(), ADDRESS)
                .unwrap()
                .canonical_key(),
            "eip155:56/erc20:0x00000000000000000000000000000000000000aa"
        );
        assert_eq!(
            AssetId::erc1155(NetworkId::bsc_mainnet(), ADDRESS, "00042")
                .unwrap()
                .canonical_key(),
            "eip155:56/erc1155:0x00000000000000000000000000000000000000aa/42"
        );
    }

    #[test]
    fn rejects_malformed_addresses_and_token_ids() {
        assert_eq!(
            AddressId::parse_evm(NetworkId::bsc_mainnet(), "0x123").unwrap_err(),
            IdentifierError::InvalidEvmAddress
        );
        assert_eq!(
            AssetId::erc721(NetworkId::bsc_mainnet(), ADDRESS, "0x2a").unwrap_err(),
            IdentifierError::InvalidTokenId
        );
    }
}
