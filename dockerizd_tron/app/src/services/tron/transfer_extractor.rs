use anyhow::{Context, Result, anyhow};
use clickhouse::types::UInt256;
use serde_json::Value;

use crate::services::tron::aml::types::SimpleTransfer;
use crate::utils::tron_address::normalize_tron_address;

pub const TRX_ASSET_ID: &str = "TRX";
pub const TRC10_ASSET_PREFIX: &str = "TRC10:";

const STANDARD_TRANSFER_TOPIC: &str =
    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const MULTI_TOKEN_TRANSFER_SINGLE_TOPIC: &str =
    "c3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62";
const MULTI_TOKEN_TRANSFER_BATCH_TOPIC: &str =
    "4a39dc06d4c0dbc64b70af90fd698a233a518aa5d07e595d983b8c0526c8f7fb";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StandardTransferEvidence {
    pub has_trc20_transfer: bool,
    pub has_nft_transfer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Native,
    Trc10,
    Trc20,
    Internal,
}

impl TransferKind {
    pub fn relationship_type(self) -> &'static str {
        match self {
            Self::Native => "native_transfer",
            Self::Trc10 => "trc10_transfer",
            Self::Trc20 => "trc20_transfer",
            Self::Internal => "internal_transfer",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedTransfer {
    pub transfer_id: String,
    pub asset_id: String,
    pub from_address: String,
    pub to_address: String,
    pub amount: UInt256,
    pub kind: TransferKind,
}

impl ExtractedTransfer {
    pub fn as_simple_transfer(&self) -> SimpleTransfer {
        SimpleTransfer {
            token: self.asset_id.clone(),
            from: self.from_address.clone(),
            to: self.to_address.clone(),
            raw_amount: self.amount,
        }
    }
}

pub fn extract_contract_transfers(tx: &Value, tx_hash: &str) -> Vec<ExtractedTransfer> {
    tx["raw_data"]["contract"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(contract_index, contract)| {
            let contract_type = contract["type"].as_str().unwrap_or_default();
            let value = &contract["parameter"]["value"];
            let Some(from_address) = normalized_field(value, "owner_address") else {
                return Vec::new();
            };

            match contract_type {
                "TransferContract" => transfer_to_address(value, "to_address", "amount")
                    .map_or_else(Vec::new, |leg| {
                        vec![new_transfer(
                            TransferSource::contract(tx_hash, contract_index, 0),
                            TRX_ASSET_ID.to_string(),
                            from_address,
                            leg.0,
                            leg.1,
                            TransferKind::Native,
                        )]
                    }),
                "TransferAssetContract" => {
                    let Some(asset_id) =
                        trc10_asset_id(value["asset_name"].as_str().unwrap_or_default())
                    else {
                        return Vec::new();
                    };

                    transfer_to_address(value, "to_address", "amount").map_or_else(
                        Vec::new,
                        |leg| {
                            vec![new_transfer(
                                TransferSource::contract(tx_hash, contract_index, 0),
                                asset_id,
                                from_address,
                                leg.0,
                                leg.1,
                                TransferKind::Trc10,
                            )]
                        },
                    )
                }
                "TriggerSmartContract" => {
                    let Some(to_address) = normalized_field(value, "contract_address") else {
                        return Vec::new();
                    };
                    let mut transfers = Vec::with_capacity(2);

                    if let Some(amount) = positive_u64(&value["call_value"]) {
                        transfers.push(new_transfer(
                            TransferSource::contract(tx_hash, contract_index, 0),
                            TRX_ASSET_ID.to_string(),
                            from_address.clone(),
                            to_address.clone(),
                            amount,
                            TransferKind::Native,
                        ));
                    }

                    if let (Some(amount), Some(token_id)) = (
                        positive_u64(&value["call_token_value"]),
                        trc10_asset_id_value(&value["token_id"]),
                    ) {
                        transfers.push(new_transfer(
                            TransferSource::contract(tx_hash, contract_index, 1),
                            token_id,
                            from_address,
                            to_address,
                            amount,
                            TransferKind::Trc10,
                        ));
                    }

                    transfers
                }
                _ => Vec::new(),
            }
        })
        .collect()
}

pub fn extract_trc20_transfers(receipt: &Value, tx_hash: &str) -> Result<Vec<ExtractedTransfer>> {
    let mut transfers = Vec::new();

    for (log_index, log) in receipt["log"].as_array().into_iter().flatten().enumerate() {
        let Some(topics) = log["topics"].as_array().filter(|topics| topics.len() >= 3) else {
            continue;
        };
        let topic0 = topics[0]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches("0x")
            .trim_start_matches("0X");

        if !topic0.eq_ignore_ascii_case(STANDARD_TRANSFER_TOPIC) {
            continue;
        }

        let Some(asset_id) = log["address"].as_str().and_then(normalize_tron_address) else {
            continue;
        };
        let Some(from_address) = topics[1].as_str().and_then(normalize_tron_address) else {
            continue;
        };
        let Some(to_address) = topics[2].as_str().and_then(normalize_tron_address) else {
            continue;
        };
        let amount = parse_uint256_hex(log["data"].as_str().unwrap_or("0x0"))
            .with_context(|| format!("invalid TRC20 amount at log index {log_index}"))?;

        if amount == UInt256::ZERO {
            continue;
        }

        transfers.push(ExtractedTransfer {
            transfer_id: format!("{tx_hash}:log:{log_index}"),
            asset_id,
            from_address,
            to_address,
            amount,
            kind: TransferKind::Trc20,
        });
    }

    Ok(transfers)
}

pub fn extract_internal_transfers(receipt: &Value, tx_hash: &str) -> Vec<ExtractedTransfer> {
    receipt["internal_transactions"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(internal_index, internal)| {
            if internal["rejected"].as_bool().unwrap_or(false) {
                return Vec::new();
            }

            let Some(from_address) = normalized_field(internal, "caller_address") else {
                return Vec::new();
            };
            let Some(to_address) = normalized_field(internal, "transferTo_address") else {
                return Vec::new();
            };

            internal["callValueInfo"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(value_index, call_value)| {
                    let amount = positive_u64(&call_value["callValue"])?;
                    let token_id = &call_value["tokenId"];
                    let is_native = token_id.is_null()
                        || token_id
                            .as_str()
                            .is_some_and(|token| token.is_empty() || token == "_");
                    let asset_id = if is_native {
                        TRX_ASSET_ID.to_string()
                    } else {
                        trc10_asset_id_value(token_id)?
                    };

                    Some(new_transfer(
                        TransferSource::internal(tx_hash, internal_index, value_index),
                        asset_id,
                        from_address.clone(),
                        to_address.clone(),
                        amount,
                        TransferKind::Internal,
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn primary_contract_summary(tx: &Value) -> (String, String, String, String, UInt256) {
    let contracts = tx["raw_data"]["contract"].as_array();
    let contract_type = match contracts {
        Some(contracts) if contracts.len() > 1 => "MultiContract".to_string(),
        Some(contracts) => contracts
            .first()
            .and_then(|contract| contract["type"].as_str())
            .unwrap_or("Unknown")
            .to_string(),
        None => "Unknown".to_string(),
    };
    let primary_contract = contracts
        .and_then(|contracts| {
            contracts
                .iter()
                .find(|contract| contract["type"] == "TriggerSmartContract")
                .or_else(|| contracts.first())
        })
        .map(|contract| &contract["parameter"]["value"]);
    let owner = primary_contract
        .and_then(|value| normalized_field(value, "owner_address"))
        .unwrap_or_default();
    let contract_address = primary_contract
        .and_then(|value| normalized_field(value, "contract_address"))
        .unwrap_or_default();
    let first_transfer = extract_contract_transfers(tx, "summary").into_iter().next();
    let to_address = first_transfer
        .as_ref()
        .map(|transfer| transfer.to_address.clone())
        .filter(|address| !address.is_empty())
        .unwrap_or_else(|| contract_address.clone());
    let amount = first_transfer
        .as_ref()
        .map(|transfer| transfer.amount)
        .unwrap_or(UInt256::ZERO);

    (contract_type, owner, to_address, contract_address, amount)
}

pub fn primary_method_data(tx: &Value) -> Option<String> {
    tx["raw_data"]["contract"]
        .as_array()?
        .iter()
        .find(|contract| contract["type"] == "TriggerSmartContract")
        .or_else(|| tx["raw_data"]["contract"].as_array()?.first())
        .and_then(|contract| contract["parameter"]["value"]["data"].as_str())
        .map(ToString::to_string)
}

pub fn has_contract_call(tx: &Value) -> bool {
    tx["raw_data"]["contract"]
        .as_array()
        .is_some_and(|contracts| {
            contracts
                .iter()
                .any(|contract| contract["type"] == "TriggerSmartContract")
        })
}

pub fn standard_transfer_evidence(
    receipt: &Value,
    target_contract: &str,
) -> StandardTransferEvidence {
    let Some(target_contract) = normalize_tron_address(target_contract) else {
        return StandardTransferEvidence::default();
    };
    let mut evidence = StandardTransferEvidence::default();

    for log in receipt["log"].as_array().into_iter().flatten() {
        let Some(log_contract) = log["address"].as_str().and_then(normalize_tron_address) else {
            continue;
        };
        if log_contract != target_contract {
            continue;
        }

        let Some(topics) = log["topics"].as_array() else {
            continue;
        };
        let topic0 = topics
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim_start_matches("0x")
            .trim_start_matches("0X");

        if topic0.eq_ignore_ascii_case(STANDARD_TRANSFER_TOPIC) {
            if topics.len() == 3 {
                evidence.has_trc20_transfer = true;
            } else if topics.len() >= 4 {
                evidence.has_nft_transfer = true;
            }
        } else if topic0.eq_ignore_ascii_case(MULTI_TOKEN_TRANSFER_SINGLE_TOPIC)
            || topic0.eq_ignore_ascii_case(MULTI_TOKEN_TRANSFER_BATCH_TOPIC)
        {
            evidence.has_nft_transfer = true;
        }
    }

    evidence
}

struct TransferSource<'a> {
    tx_hash: &'a str,
    name: &'static str,
    source_index: usize,
    value_index: usize,
}

impl<'a> TransferSource<'a> {
    fn contract(tx_hash: &'a str, source_index: usize, value_index: usize) -> Self {
        Self {
            tx_hash,
            name: "contract",
            source_index,
            value_index,
        }
    }

    fn internal(tx_hash: &'a str, source_index: usize, value_index: usize) -> Self {
        Self {
            tx_hash,
            name: "internal",
            source_index,
            value_index,
        }
    }
}

fn new_transfer(
    source: TransferSource<'_>,
    asset_id: String,
    from_address: String,
    to_address: String,
    amount: u64,
    kind: TransferKind,
) -> ExtractedTransfer {
    ExtractedTransfer {
        transfer_id: format!(
            "{}:{}:{}:{}",
            source.tx_hash, source.name, source.source_index, source.value_index
        ),
        asset_id,
        from_address,
        to_address,
        amount: UInt256::from(amount),
        kind,
    }
}

fn transfer_to_address(
    value: &Value,
    address_key: &str,
    amount_key: &str,
) -> Option<(String, u64)> {
    let to_address = normalized_field(value, address_key)?;
    let amount = positive_u64(&value[amount_key])?;

    Some((to_address, amount))
}

fn normalized_field(value: &Value, field: &str) -> Option<String> {
    value[field].as_str().and_then(normalize_tron_address)
}

fn positive_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|amount| *amount > 0)
}

fn trc10_asset_id(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "_" {
        return None;
    }

    let decoded_bytes = (raw.len().is_multiple_of(2)
        && raw.chars().all(|ch| ch.is_ascii_hexdigit()))
    .then(|| hex::decode(raw).ok())
    .flatten();
    let decoded_hex = decoded_bytes
        .as_ref()
        .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
        .filter(|decoded| decoded.chars().all(|ch| ch.is_ascii_digit()));
    if let Some(decoded) = decoded_hex {
        return Some(format!("{TRC10_ASSET_PREFIX}{}", decoded.trim()));
    }
    if raw.chars().all(|ch| ch.is_ascii_digit()) {
        return Some(format!("{TRC10_ASSET_PREFIX}{raw}"));
    }

    Some(decoded_bytes.map_or_else(
        || {
            format!(
                "{TRC10_ASSET_PREFIX}legacy:{}",
                hex::encode_upper(raw.as_bytes())
            )
        },
        |bytes| format!("{TRC10_ASSET_PREFIX}legacy:{}", hex::encode_upper(bytes)),
    ))
}

fn trc10_asset_id_value(value: &Value) -> Option<String> {
    value
        .as_u64()
        .map(|token_id| format!("{TRC10_ASSET_PREFIX}{token_id}"))
        .or_else(|| trc10_asset_id(value.as_str().unwrap_or_default()))
}

fn parse_uint256_hex(value: &str) -> Result<UInt256> {
    let value = value
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches('0');

    if value.len() > 64 {
        return Err(anyhow!("uint256 value exceeds 32 bytes"));
    }
    if value.is_empty() {
        return Ok(UInt256::ZERO);
    }

    let normalized = if value.len().is_multiple_of(2) {
        value.to_string()
    } else {
        format!("0{value}")
    };
    let big_endian = hex::decode(&normalized).context("invalid uint256 hex value")?;
    let mut little_endian = [0u8; 32];

    for (index, byte) in big_endian.iter().rev().enumerate() {
        little_endian[index] = *byte;
    }

    Ok(UInt256::from_le_bytes(little_endian))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        TRC10_ASSET_PREFIX, TransferKind, extract_contract_transfers, extract_internal_transfers,
        extract_trc20_transfers, has_contract_call, primary_contract_summary,
        standard_transfer_evidence, trc10_asset_id,
    };

    const OWNER: &str = "4125ad4a9a23d1865faeaea080322f3e08cc205489";
    const TARGET: &str = "4130760c7e10b1d3509d8d64a7e9eb9ab94bc83495";
    const TOKEN: &str = "41a614f803b6fd780986a42c78ec9c7f77e6ded13c";

    #[test]
    fn extracts_every_top_level_contract_and_call_value() {
        let tx = json!({
            "raw_data": {
                "contract": [
                    {
                        "type": "TransferContract",
                        "parameter": {"value": {
                            "owner_address": OWNER,
                            "to_address": TARGET,
                            "amount": 12
                        }}
                    },
                    {
                        "type": "TransferAssetContract",
                        "parameter": {"value": {
                            "owner_address": OWNER,
                            "to_address": TARGET,
                            "asset_name": "31303032303030",
                            "amount": 34
                        }}
                    },
                    {
                        "type": "TriggerSmartContract",
                        "parameter": {"value": {
                            "owner_address": OWNER,
                            "contract_address": TOKEN,
                            "call_value": 56,
                            "call_token_value": 78,
                            "token_id": "1002001",
                            "data": "abcdef01"
                        }}
                    }
                ]
            }
        });

        let transfers = extract_contract_transfers(&tx, "tx");

        assert_eq!(transfers.len(), 4);
        assert_eq!(transfers[0].kind, TransferKind::Native);
        assert_eq!(
            transfers[1].asset_id,
            format!("{TRC10_ASSET_PREFIX}1002000")
        );
        assert_eq!(
            transfers[3].asset_id,
            format!("{TRC10_ASSET_PREFIX}1002001")
        );
        assert!(has_contract_call(&tx));
        assert_eq!(primary_contract_summary(&tx).0, "MultiContract");
    }

    #[test]
    fn extracts_successful_internal_native_and_trc10_values() {
        let receipt = json!({
            "internal_transactions": [
                {
                    "caller_address": OWNER,
                    "transferTo_address": TARGET,
                    "rejected": false,
                    "callValueInfo": [
                        {"callValue": 90, "tokenId": "_"},
                        {"callValue": 12, "tokenId": "1002001"}
                    ]
                },
                {
                    "caller_address": OWNER,
                    "transferTo_address": TARGET,
                    "rejected": true,
                    "callValueInfo": [{"callValue": 999, "tokenId": "_"}]
                }
            ]
        });

        let transfers = extract_internal_transfers(&receipt, "tx");

        assert_eq!(transfers.len(), 2);
        assert!(
            transfers
                .iter()
                .all(|transfer| transfer.kind == TransferKind::Internal)
        );
        assert_eq!(transfers[0].asset_id, "TRX");
        assert_eq!(transfers[1].asset_id, "TRC10:1002001");
    }

    #[test]
    fn extracts_trc20_transfer_without_persisting_raw_log() {
        let receipt = json!({
            "log": [{
                "address": TOKEN,
                "topics": [
                    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                    format!("{:0>64}", &OWNER[2..]),
                    format!("{:0>64}", &TARGET[2..])
                ],
                "data": "0000000000000000000000000000000000000000000000000000000000000064"
            }]
        });

        let transfers = extract_trc20_transfers(&receipt, "tx").expect("valid transfer");

        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].kind, TransferKind::Trc20);
        assert_eq!(transfers[0].transfer_id, "tx:log:0");
        assert_eq!(transfers[0].amount, 100u64.into());
    }

    #[test]
    fn standard_transfer_evidence_only_uses_logs_from_the_target_contract() {
        let receipt = json!({
            "log": [
                {
                    "address": TOKEN,
                    "topics": [
                        "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                        format!("{:0>64}", &OWNER[2..]),
                        format!("{:0>64}", &TARGET[2..])
                    ],
                    "data": "01"
                },
                {
                    "address": TOKEN,
                    "topics": [
                        "c3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62",
                        format!("{:0>64}", &OWNER[2..]),
                        format!("{:0>64}", &OWNER[2..]),
                        format!("{:0>64}", &TARGET[2..])
                    ],
                    "data": "01"
                },
                {
                    "address": TARGET,
                    "topics": [
                        "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                        format!("{:0>64}", &OWNER[2..]),
                        format!("{:0>64}", &TARGET[2..])
                    ],
                    "data": "01"
                }
            ]
        });

        let evidence = standard_transfer_evidence(&receipt, TOKEN);

        assert!(evidence.has_trc20_transfer);
        assert!(evidence.has_nft_transfer);
    }

    #[test]
    fn preserves_legacy_named_trc10_asset_identity() {
        assert_eq!(
            trc10_asset_id("57494e").as_deref(),
            Some("TRC10:legacy:57494E")
        );
    }
}
