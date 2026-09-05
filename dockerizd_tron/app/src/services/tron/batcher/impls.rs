use crate::models::tron::modules::{
    SemanticAmlEventRow, TokenMetadataDiscoveryRow, TransactionFeatureRow, TransactionRow,
};
use crate::models::tron::relationship::AddressRelationshipRow;

use super::traits::BatchInsert;

impl BatchInsert for TransactionRow {
    const TABLE: &'static str = "transactions";

    fn as_value(&self) -> Self::Value<'_> {
        self.clone()
    }
}

impl BatchInsert for TokenMetadataDiscoveryRow {
    const TABLE: &'static str = "token_metadata_discoveries";

    fn as_value(&self) -> Self::Value<'_> {
        self.clone()
    }
}

impl BatchInsert for AddressRelationshipRow {
    const TABLE: &'static str = "address_relationships";

    fn as_value(&self) -> Self::Value<'_> {
        self.clone()
    }
}

impl BatchInsert for SemanticAmlEventRow {
    const TABLE: &'static str = "semantic_aml_events";

    fn as_value(&self) -> Self::Value<'_> {
        self.clone()
    }
}

impl BatchInsert for TransactionFeatureRow {
    const TABLE: &'static str = "transaction_features";

    fn as_value(&self) -> Self::Value<'_> {
        self.clone()
    }
}
