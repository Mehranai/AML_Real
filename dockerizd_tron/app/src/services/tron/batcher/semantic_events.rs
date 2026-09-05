use crate::models::tron::modules::SemanticAmlEventRow;

use super::generic::GenericBatcher;

pub type SemanticEventBatcher = GenericBatcher<SemanticAmlEventRow>;
