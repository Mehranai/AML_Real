use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Tron,
    Ethereum,
    Bsc,
    Bitcoin,
}
impl Network {
    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|network| network.id() == value)
            .ok_or_else(|| anyhow::anyhow!("unknown warehouse network identity"))
    }
    pub const fn native_asset(self) -> &'static str {
        match self {
            Self::Tron => "native:TRX",
            Self::Ethereum => "native:ETH",
            Self::Bsc => "native:BNB",
            Self::Bitcoin => "native:BTC",
        }
    }
    pub const fn native_decimals(self) -> u8 {
        match self {
            Self::Tron => 6,
            Self::Ethereum | Self::Bsc => 18,
            Self::Bitcoin => 8,
        }
    }
    pub const ALL: [Self; 4] = [Self::Tron, Self::Ethereum, Self::Bsc, Self::Bitcoin];
    pub const fn id(self) -> &'static str {
        match self {
            Self::Tron => "tron:mainnet",
            Self::Ethereum => "eip155:1",
            Self::Bsc => "eip155:56",
            Self::Bitcoin => "bip122:000000000019d6689c085ae165831e93",
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tron => "tron",
            Self::Ethereum => "ethereum",
            Self::Bsc => "bsc",
            Self::Bitcoin => "bitcoin",
        }
    }
}

/// Raw asset quantities are unsigned integers serialized as decimal strings.
/// No floating point, JSON-number rounding or chain-specific integer narrowing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct RawAmount(String);
impl RawAmount {
    pub const MAX: &'static str =
        "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    pub fn parse(raw: &str) -> Result<Self> {
        let max = Self::MAX;
        ensure!(
            !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()),
            "amount must be an unsigned decimal integer string"
        );
        let normalized = raw.trim_start_matches('0');
        let normalized = if normalized.is_empty() {
            "0"
        } else {
            normalized
        };
        ensure!(
            normalized.len() < max.len() || (normalized.len() == max.len() && normalized <= max),
            "amount exceeds UInt256"
        );
        Ok(Self(normalized.into()))
    }
    pub fn from_satoshis(sats: u64) -> Result<Self> {
        ensure!(
            sats <= 2_100_000_000_000_000,
            "satoshi amount exceeds Bitcoin supply"
        );
        Self::parse(&sats.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Convert decimal asset units using strings only; never round lost digits.
    pub fn from_decimal(value: &str, decimals: u8) -> Result<Self> {
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        ensure!(
            !whole.is_empty()
                && whole.bytes().all(|b| b.is_ascii_digit())
                && fraction.bytes().all(|b| b.is_ascii_digit())
                && (!value.contains('.') || !fraction.is_empty()),
            "invalid exact decimal amount"
        );
        let significant = fraction.trim_end_matches('0');
        ensure!(
            significant.len() <= decimals as usize,
            "amount has sub-atomic precision"
        );
        let zeros = (decimals as usize).saturating_sub(significant.len());
        Self::parse(&format!("{whole}{significant}{}", "0".repeat(zeros)))
    }
    pub fn decimal(&self, decimals: u8) -> String {
        if decimals == 0 {
            return self.0.clone();
        }
        let padded = format!("{:0>width$}", self.0, width = decimals as usize + 1);
        let split = padded.len() - decimals as usize;
        format!("{}.{}", &padded[..split], &padded[split..])
    }
}
impl<'de> Deserialize<'de> for RawAmount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMillis(pub u64);
impl UnixMillis {
    pub fn from_seconds(seconds: u64) -> Result<Self> {
        Ok(Self(
            seconds
                .checked_mul(1000)
                .ok_or_else(|| anyhow::anyhow!("timestamp overflow"))?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn amounts_preserve_uint256_and_one_satoshi() {
        let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        assert_eq!(RawAmount::parse(max).unwrap().as_str(), max);
        assert_eq!(RawAmount::from_satoshis(1).unwrap().as_str(), "1");
        assert_eq!(RawAmount::parse("00010").unwrap().as_str(), "10");
        for invalid in [
            "",
            "1e18",
            "-1",
            "1.1",
            " 1",
            "115792089237316195423570985008687907853269984665640564039457584007913129639936",
        ] {
            assert!(RawAmount::parse(invalid).is_err());
        }
        assert!(serde_json::from_str::<RawAmount>("100").is_err());
        assert_eq!(
            serde_json::to_string(&RawAmount::parse(max).unwrap()).unwrap(),
            format!("\"{max}\"")
        );
    }
    #[test]
    fn timestamps_are_explicit_and_checked() {
        assert_eq!(UnixMillis::from_seconds(123).unwrap().0, 123000);
        assert!(UnixMillis::from_seconds(u64::MAX).is_err());
    }
}
