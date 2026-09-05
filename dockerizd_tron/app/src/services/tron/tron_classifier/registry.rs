use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::Deserialize;

use super::types::{ContractCategory, ProtocolInfo};

const OFFICIAL_REGISTRY_SOURCE: &str = "official_builtin_registry";
const APPROVED_REGISTRY_SOURCE: &str = "approved_entity_registry";

pub struct ProtocolRegistry {
    entries: RwLock<HashMap<String, ProtocolInfo>>,
    refresh_every_blocks: u64,
    last_refresh_block: AtomicU64,
}

#[derive(Debug, Deserialize, Row)]
struct ActiveEntityRow {
    address: String,
    entity_name: String,
    entity_type: String,
    confidence: f32,
}

impl ProtocolRegistry {
    pub async fn load(client: &Client, refresh_every_blocks: u64) -> Result<Self> {
        let registry = Self {
            entries: RwLock::new(official_protocols()),
            refresh_every_blocks,
            last_refresh_block: AtomicU64::new(0),
        };
        registry.refresh(client).await?;
        Ok(registry)
    }

    pub fn lookup(&self, address: &str) -> Option<ProtocolInfo> {
        if address.trim().is_empty() {
            return None;
        }

        self.entries
            .read()
            .expect("protocol registry lock poisoned")
            .get(address.trim())
            .cloned()
    }

    pub async fn refresh_if_due(&self, client: &Client, block_number: u64) -> Result<bool> {
        if self.refresh_every_blocks == 0 {
            return Ok(false);
        }

        let last = self.last_refresh_block.load(Ordering::Acquire);
        if block_number < last || block_number.saturating_sub(last) < self.refresh_every_blocks {
            return Ok(false);
        }

        self.refresh(client).await?;
        self.last_refresh_block
            .store(block_number, Ordering::Release);
        Ok(true)
    }

    async fn refresh(&self, client: &Client) -> Result<()> {
        let rows = client
            .query(
                r#"
                SELECT
                    address,
                    entity_name,
                    entity_type,
                    confidence
                FROM address_entity FINAL
                WHERE is_active = 1
                  AND address != ''
                "#,
            )
            .fetch_all::<ActiveEntityRow>()
            .await
            .context("failed to refresh the approved TRON protocol registry")?;

        let mut refreshed = official_protocols();
        for row in rows {
            let Some(category) = category_from_entity_type(&row.entity_type) else {
                continue;
            };

            refreshed.insert(
                row.address,
                ProtocolInfo {
                    protocol: row.entity_name,
                    category,
                    confidence: row.confidence.clamp(0.0, 1.0),
                    source: APPROVED_REGISTRY_SOURCE.to_string(),
                },
            );
        }

        *self
            .entries
            .write()
            .expect("protocol registry lock poisoned") = refreshed;
        Ok(())
    }

    #[cfg(test)]
    pub fn from_entries(entries: impl IntoIterator<Item = (String, ProtocolInfo)>) -> Self {
        Self {
            entries: RwLock::new(entries.into_iter().collect()),
            refresh_every_blocks: 0,
            last_refresh_block: AtomicU64::new(0),
        }
    }
}

pub fn category_from_entity_type(entity_type: &str) -> Option<ContractCategory> {
    match entity_type.trim().to_ascii_lowercase().as_str() {
        "dex" | "amm" | "decentralized_exchange" | "dex_router" | "dex_pool" => {
            Some(ContractCategory::Dex)
        }
        "bridge" | "cross_chain_bridge" | "bridge_router" | "bridge_vault" => {
            Some(ContractCategory::Bridge)
        }
        "lending" | "lending_protocol" | "lending_market" | "lending_vault" => {
            Some(ContractCategory::Lending)
        }
        "staking" | "staking_protocol" | "liquid_staking" | "staking_pool" => {
            Some(ContractCategory::Staking)
        }
        "mixer" | "tumbler" | "privacy_pool" => Some(ContractCategory::Mixer),
        "token" | "token_contract" | "stablecoin" | "wrapped_token" => {
            Some(ContractCategory::Token)
        }
        "nft" | "nft_collection" | "nft_marketplace" => Some(ContractCategory::Nft),
        "scam" | "fraud" | "phishing" | "ransomware" | "malware" => Some(ContractCategory::Scam),
        "wallet"
        | "service_wallet"
        | "individual"
        | "exchange"
        | "centralized_exchange"
        | "custodial_exchange"
        | "custodian" => Some(ContractCategory::Wallet),
        _ => None,
    }
}

// Bootstrap deployment sources:
// https://docs.sun.io/protocols/sunswap-v2/reference/contract/
// https://docs.justlend.org/developers/deployed_contracts/
// Reviewed address_entity rows override these entries at runtime.
fn official_protocols() -> HashMap<String, ProtocolInfo> {
    let mut entries = HashMap::new();

    insert(
        &mut entries,
        "TKWJdrQkqHisa1X8HUdHEfREvTzw4pMAaY",
        "SunSwap V2 Factory",
        ContractCategory::Dex,
    );
    insert(
        &mut entries,
        "TNJVzGqKBWkJxJB5XYSqGAwUTV15U24pPq",
        "SunSwap V2 Router",
        ContractCategory::Dex,
    );
    insert(
        &mut entries,
        "TKzxdSv2FZKQrEqkKVgp5DcwEXBEKMg2Ax",
        "SunSwap V2 Router (historical)",
        ContractCategory::Dex,
    );
    insert(
        &mut entries,
        "TMmRKgwC1hx4azzkXoPzujgQThzF48Actq",
        "SunSwap V2 Router (deprecated)",
        ContractCategory::Dex,
    );
    insert(
        &mut entries,
        "TXF1xDbVGdxFGbovmmmXvBGu8ZiE3Lq4mR",
        "SunSwap legacy contract (deprecated)",
        ContractCategory::Dex,
    );

    insert(
        &mut entries,
        "TGjYzgCyPobsNS9n6WcbdLVR9dH7mWqFx7",
        "JustLend Unitroller",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TE2RzoSV3wFK99w6J9UnnZ4vLfXYoxvRwP",
        "JustLend jTRX",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TXJgMdjVX5dKiQaUi9QobwNxtSQaFqccvd",
        "JustLend jUSDT",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TKFRELGGoRgiayhwJTNNLqCNjFoLBh3Mnf",
        "JustLend jUSDD",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TDH4dhmVQQNc1ZNudJwWzBcs2h6ahhWrpp",
        "JustLend V2 Moolah",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TMDENHFSiRzmJNSEBAFmrDbLkQ672iPN8H",
        "JustLend V2 TRX Provider",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TXejU9jmd1ooQyY3Zmpo15yN7MjSFYUESg",
        "JustLend V2 USDT Vault",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TA3q7XjdBQWb4qFxaPULUsnjvVZGgC9Brz",
        "JustLend V2 USDD Vault",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "THpxp8RpCUGk55dV7oL1LfxDeP9QvouxmM",
        "JustLend V2 TRX Vault",
        ContractCategory::Lending,
    );
    insert(
        &mut entries,
        "TU3kjFuhtEo42tsCBtfYUAZxoqQ4yuSLQ5",
        "JustLend sTRX",
        ContractCategory::Staking,
    );

    insert(
        &mut entries,
        "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t",
        "Tether USD",
        ContractCategory::Token,
    );
    insert(
        &mut entries,
        "TXDk8mbtRbXeYuMNS83CfKPaYYT8XWv9Hz",
        "USDD",
        ContractCategory::Token,
    );
    insert(
        &mut entries,
        "TMwFHYXLJaRUPeW6421aqXL4ZEzPRFGkGT",
        "USDJ",
        ContractCategory::Token,
    );
    insert(
        &mut entries,
        "TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR",
        "Wrapped TRX",
        ContractCategory::Token,
    );

    entries
}

fn insert(
    entries: &mut HashMap<String, ProtocolInfo>,
    address: &str,
    protocol: &str,
    category: ContractCategory,
) {
    entries.insert(
        address.to_string(),
        ProtocolInfo {
            protocol: protocol.to_string(),
            category,
            confidence: 0.99,
            source: OFFICIAL_REGISTRY_SOURCE.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_registry_does_not_mislabel_usdj_as_lending() {
        let entries = official_protocols();
        let usdj = entries
            .get("TMwFHYXLJaRUPeW6421aqXL4ZEzPRFGkGT")
            .expect("USDJ must be registered");

        assert_eq!(usdj.category, ContractCategory::Token);
    }

    #[test]
    fn governed_entity_types_cover_every_reviewable_category() {
        assert_eq!(
            category_from_entity_type("decentralized_exchange"),
            Some(ContractCategory::Dex)
        );
        assert_eq!(
            category_from_entity_type("cross_chain_bridge"),
            Some(ContractCategory::Bridge)
        );
        assert_eq!(
            category_from_entity_type("lending_protocol"),
            Some(ContractCategory::Lending)
        );
        assert_eq!(
            category_from_entity_type("liquid_staking"),
            Some(ContractCategory::Staking)
        );
        assert_eq!(
            category_from_entity_type("mixer"),
            Some(ContractCategory::Mixer)
        );
        assert_eq!(
            category_from_entity_type("stablecoin"),
            Some(ContractCategory::Token)
        );
        assert_eq!(
            category_from_entity_type("nft_collection"),
            Some(ContractCategory::Nft)
        );
        assert_eq!(
            category_from_entity_type("phishing"),
            Some(ContractCategory::Scam)
        );
        assert_eq!(
            category_from_entity_type("centralized_exchange"),
            Some(ContractCategory::Wallet)
        );
    }
}
