//! Canonical values at the shared storage boundary. Legacy normalization is
//! explicit: validation of an already canonical row never silently rewrites it.
use crate::{Network, RawAmount};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

pub fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn normalize_hash(network: Network, value: &str) -> Result<String> {
    let evm = matches!(network, Network::Ethereum | Network::Bsc);
    let digits = if evm {
        value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value)
    } else {
        value
    };
    ensure!(hex(digits, 64), "invalid network hash");
    Ok(format!(
        "{}{}",
        if evm { "0x" } else { "" },
        digits.to_ascii_lowercase()
    ))
}

fn tron_contract(value: &str) -> Result<String> {
    if value.starts_with("41") && hex(value, 42) {
        return Ok(value.to_ascii_lowercase());
    }
    ensure!(
        value.len() == 34 && value.starts_with('T'),
        "invalid TRON contract identity"
    );
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut bytes = [0u8; 25];
    for character in value.bytes() {
        let mut carry = alphabet
            .iter()
            .position(|b| *b == character)
            .ok_or_else(|| anyhow::anyhow!("invalid base58 character"))?
            as u32;
        for byte in bytes.iter_mut().rev() {
            carry += u32::from(*byte) * 58;
            *byte = carry as u8;
            carry >>= 8;
        }
        ensure!(carry == 0, "TRON address overflow");
    }
    let checksum = Sha256::digest(Sha256::digest(&bytes[..21]));
    ensure!(
        bytes[0] == 0x41 && bytes[21..] == checksum[..4],
        "TRON contract checksum/network mismatch"
    );
    Ok(bytes[..21].iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetId {
    pub network: Network,
    value: String,
    pub collection: bool,
}
impl AssetId {
    pub fn parse_legacy(network: Network, raw: &str) -> Result<Self> {
        let value = if let Some((prefix, rest)) = raw.split_once('/') {
            if prefix.starts_with("eip155:")
                || prefix.starts_with("bip122:")
                || prefix.starts_with("tron:")
            {
                ensure!(prefix == network.id(), "asset belongs to another network");
                rest
            } else {
                raw
            }
        } else {
            raw
        };
        let native = network.native_asset();
        if value.eq_ignore_ascii_case(native) || value.eq_ignore_ascii_case(&native[7..]) {
            return Ok(Self {
                network,
                value: native.into(),
                collection: false,
            });
        }
        ensure!(
            network != Network::Bitcoin,
            "Bitcoin token capability is not applicable"
        );
        let (standard, identity) = value
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("unqualified asset identity"))?;
        let standard = standard.to_ascii_lowercase();
        let evm = matches!(network, Network::Ethereum | Network::Bsc);
        ensure!(
            if evm {
                ["erc20", "erc721", "erc1155"].contains(&standard.as_str())
            } else {
                ["trc10", "trc20", "trc721"].contains(&standard.as_str())
            },
            "asset standard not applicable to network"
        );
        let (contract, token) = identity
            .split_once('/')
            .map_or((identity, None), |(c, t)| (c, Some(t)));
        let contract = if standard == "trc10" {
            ensure!(token.is_none(), "TRC10 does not carry an NFT token id");
            RawAmount::parse(contract)?.as_str().to_owned()
        } else if evm {
            let digits = contract
                .strip_prefix("0x")
                .or_else(|| contract.strip_prefix("0X"))
                .unwrap_or(contract);
            ensure!(hex(digits, 40), "invalid EVM asset contract");
            format!("0x{}", digits.to_ascii_lowercase())
        } else {
            tron_contract(contract)?
        };
        let nft = ["erc721", "erc1155", "trc721"].contains(&standard.as_str());
        ensure!(
            nft || token.is_none(),
            "fungible asset cannot carry a token id"
        );
        let suffix = if let Some(token) = token {
            format!("/{}", RawAmount::parse(token)?.as_str())
        } else {
            String::new()
        };
        Ok(Self {
            network,
            value: format!("{standard}:{contract}{suffix}"),
            collection: nft && token.is_none(),
        })
    }
    pub fn parse(network: Network, value: &str) -> Result<Self> {
        let asset = Self::parse_legacy(network, value)?;
        ensure!(
            asset.value == value,
            "asset must use canonical network-local identity"
        );
        Ok(asset)
    }
    pub fn as_str(&self) -> &str {
        &self.value
    }
    pub fn canonical_key(&self) -> String {
        format!("{}/{}", self.network.id(), self.value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewStatus {
    Pending,
    Approved,
    Rejected,
    Revoked,
}
impl ReviewStatus {
    pub fn parse_legacy(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "revoked" => Ok(Self::Revoked),
            _ => anyhow::bail!("unknown review state; verification is not approval"),
        }
    }
}

pub fn evidence_ref(network: Network, value: &str) -> Result<()> {
    ensure!(
        !value.chars().any(char::is_control),
        "evidence reference contains control characters"
    );
    let (scheme, payload) = value
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("evidence reference needs a namespace"))?;
    ensure!(
        !scheme.is_empty()
            && scheme.bytes().enumerate().all(
                |(i, b)| b.is_ascii_lowercase() || (i > 0 && (b.is_ascii_digit() || b == b'_'))
            )
            && !payload.is_empty(),
        "invalid evidence namespace/payload"
    );
    if matches!(scheme, "tx" | "block") {
        ensure!(
            normalize_hash(network, payload)? == payload,
            "noncanonical evidence hash"
        );
    } else if scheme == "outpoint" {
        ensure!(
            network == Network::Bitcoin,
            "outpoint evidence requires Bitcoin"
        );
        let (hash, index) = payload
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("outpoint index missing"))?;
        ensure!(
            normalize_hash(network, hash)? == hash,
            "noncanonical outpoint hash"
        );
        let parsed: u32 = index.parse()?;
        ensure!(parsed.to_string() == index, "noncanonical outpoint index");
    }
    Ok(())
}
