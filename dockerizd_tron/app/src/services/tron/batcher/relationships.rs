use crate::models::tron::relationship::AddressRelationshipRow;

use super::generic::GenericBatcher;

pub type RelationshipBatcher = GenericBatcher<AddressRelationshipRow>;
