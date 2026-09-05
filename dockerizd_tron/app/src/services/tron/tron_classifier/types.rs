use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContractCategory {
    Dex,
    Bridge,
    Lending,
    Staking,
    Mixer,
    Token,
    Nft,
    Scam,
    Wallet,
    Unknown,
}

impl fmt::Display for ContractCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let category = match self {
            Self::Dex => "DEX",
            Self::Bridge => "BRIDGE",
            Self::Lending => "LENDING",
            Self::Staking => "STAKING",
            Self::Mixer => "MIXER",
            Self::Token => "TOKEN",
            Self::Nft => "NFT",
            Self::Scam => "SCAM",
            Self::Wallet => "WALLET",
            Self::Unknown => "UNKNOWN",
        };

        f.write_str(category)
    }
}

#[derive(Debug, Clone)]
pub struct ClassificationInput {
    pub contract_address: String,
    pub target_address: String,
    pub contract_type: String,
    pub method_data: Option<String>,
    pub has_trc20_transfer: bool,
    pub has_trc721_transfer: bool,
}

#[derive(Debug, Clone)]
pub struct ProtocolInfo {
    pub protocol: String,
    pub category: ContractCategory,
    pub confidence: f32,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub protocol: String,
    pub category: ContractCategory,
    pub confidence: f32,
    pub detection_source: String,
    pub method_id: Option<String>,
    pub operation: Option<String>,
}
