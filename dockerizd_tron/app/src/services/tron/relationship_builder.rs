use crate::models::tron::relationship::AddressRelationshipRow;
use crate::services::tron::transfer_extractor::ExtractedTransfer;

pub fn build_relationships(
    tx_hash: &str,
    block_number: u64,
    timestamp: u64,
    transfers: &[ExtractedTransfer],
) -> Vec<AddressRelationshipRow> {
    transfers
        .iter()
        .map(|transfer| AddressRelationshipRow {
            relationship_id: transfer.transfer_id.clone(),
            from_address: transfer.from_address.clone(),
            to_address: transfer.to_address.clone(),
            token_address: transfer.asset_id.clone(),
            tx_hash: tx_hash.to_string(),
            block_number,
            timestamp,
            amount: transfer.amount,
            transfer_type: transfer.kind.relationship_type().to_string(),
        })
        .collect()
}
