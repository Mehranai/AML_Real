use crate::models::tron::modules::TransactionRow;

use super::generic::GenericBatcher;

pub type TransactionBatcher = GenericBatcher<TransactionRow>;
