use alloy::primitives::{B256, U256};
use sha2::{Digest, Sha256};

use super::{AddressId, AssetId, NetworkId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    NativeExternal,
    NativeInternal,
    Erc20,
    Erc20Mint,
    Erc20Burn,
    Erc721,
    Erc721Mint,
    Erc721Burn,
    Erc1155,
    Erc1155Mint,
    Erc1155Burn,
    ValidatorWithdrawal,
    ProtocolFee,
}

impl TransferKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeExternal => "native_external",
            Self::NativeInternal => "native_internal",
            Self::Erc20 => "erc20",
            Self::Erc20Mint => "erc20_mint",
            Self::Erc20Burn => "erc20_burn",
            Self::Erc721 => "erc721",
            Self::Erc721Mint => "erc721_mint",
            Self::Erc721Burn => "erc721_burn",
            Self::Erc1155 => "erc1155",
            Self::Erc1155Mint => "erc1155_mint",
            Self::Erc1155Burn => "erc1155_burn",
            Self::ValidatorWithdrawal => "validator_withdrawal",
            Self::ProtocolFee => "protocol_fee",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOrigin {
    pub tx_hash: Option<B256>,
    pub transaction_index: u32,
    pub event_index: u32,
    pub event_sub_index: u32,
    pub trace_address: Vec<u32>,
}

impl TransferOrigin {
    pub fn transaction(tx_hash: B256, transaction_index: u32) -> Self {
        Self {
            tx_hash: Some(tx_hash),
            transaction_index,
            event_index: 0,
            event_sub_index: 0,
            trace_address: Vec::new(),
        }
    }

    pub fn log(tx_hash: B256, transaction_index: u32, log_index: u32) -> Self {
        Self {
            tx_hash: Some(tx_hash),
            transaction_index,
            event_index: log_index,
            event_sub_index: 0,
            trace_address: Vec::new(),
        }
    }

    pub fn log_item(
        tx_hash: B256,
        transaction_index: u32,
        log_index: u32,
        event_sub_index: u32,
    ) -> Self {
        Self {
            tx_hash: Some(tx_hash),
            transaction_index,
            event_index: log_index,
            event_sub_index,
            trace_address: Vec::new(),
        }
    }

    pub fn trace(tx_hash: B256, transaction_index: u32, trace_address: Vec<u32>) -> Self {
        Self {
            tx_hash: Some(tx_hash),
            transaction_index,
            event_index: 0,
            event_sub_index: 0,
            trace_address,
        }
    }

    pub fn validator_withdrawal(withdrawal_index: u32) -> Self {
        Self {
            tx_hash: None,
            transaction_index: u32::MAX,
            event_index: withdrawal_index,
            event_sub_index: 0,
            trace_address: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CanonicalTransfer {
    pub relationship_id: String,
    pub network: NetworkId,
    pub block_hash: B256,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub origin: TransferOrigin,
    pub from: AddressId,
    pub to: AddressId,
    pub asset: AssetId,
    pub token_id: Option<U256>,
    pub amount: U256,
    pub kind: TransferKind,
}

impl CanonicalTransfer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        network: NetworkId,
        block_hash: B256,
        block_number: u64,
        block_timestamp_unix_ms: u64,
        origin: TransferOrigin,
        from: AddressId,
        to: AddressId,
        asset: AssetId,
        token_id: Option<U256>,
        amount: U256,
        kind: TransferKind,
    ) -> Result<Self, TransferError> {
        if from.network() != &network || to.network() != &network || asset.network() != &network {
            return Err(TransferError::NetworkMismatch);
        }
        if amount.is_zero() {
            return Err(TransferError::ZeroAmount);
        }

        let relationship_id = relationship_id(
            &network,
            block_number,
            &origin,
            &from,
            &to,
            &asset,
            token_id,
            kind,
        );

        Ok(Self {
            relationship_id,
            network,
            block_hash,
            block_number,
            block_timestamp_unix_ms,
            origin,
            from,
            to,
            asset,
            token_id,
            amount,
            kind,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn relationship_id(
    network: &NetworkId,
    block_number: u64,
    origin: &TransferOrigin,
    from: &AddressId,
    to: &AddressId,
    asset: &AssetId,
    token_id: Option<U256>,
    kind: TransferKind,
) -> String {
    let tx_hash = origin
        .tx_hash
        .map(|hash| format!("{hash:#x}"))
        .unwrap_or_else(|| "system".to_string());
    let trace_address = origin
        .trace_address
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let token_id = token_id.map(|value| value.to_string()).unwrap_or_default();
    let identity = format!(
        "{network}|{block_number}|{tx_hash}|{}|{}|{}|{trace_address}|{}|{}|{}|{token_id}|{}",
        origin.transaction_index,
        origin.event_index,
        origin.event_sub_index,
        from.canonical_key(),
        to.canonical_key(),
        asset.canonical_key(),
        kind.as_str(),
    );

    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransferError {
    #[error("all transfer participants and assets must belong to the same network")]
    NetworkMismatch,
    #[error("zero-value movements are not canonical transfer facts")]
    ZeroAmount,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::primitives::{B256, U256};

    use super::{CanonicalTransfer, TransferKind, TransferOrigin};
    use crate::domain::{AddressId, AssetId, NetworkId};

    #[test]
    fn relationship_id_is_replay_stable() {
        let network = NetworkId::ethereum_mainnet();
        let from = AddressId::parse_evm(
            network.clone(),
            "0x0000000000000000000000000000000000000001",
        )
        .unwrap();
        let to = AddressId::parse_evm(
            network.clone(),
            "0x0000000000000000000000000000000000000002",
        )
        .unwrap();
        let asset = AssetId::native_eth(network.clone());
        let tx_hash =
            B256::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .unwrap();

        let build = || {
            CanonicalTransfer::new(
                network.clone(),
                B256::ZERO,
                100,
                1_700_000_000_000,
                TransferOrigin::trace(tx_hash, 4, vec![0, 2]),
                from.clone(),
                to.clone(),
                asset.clone(),
                None,
                U256::from(10_u64),
                TransferKind::NativeInternal,
            )
            .unwrap()
        };

        assert_eq!(build().relationship_id, build().relationship_id);
    }
}
