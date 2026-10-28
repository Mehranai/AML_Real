//! DB-06 interpretation/read contract over the immutable v4 storage schema.
//! This module never rewrites legacy evidence or infers input/output allocation.
mod model;
mod query;
pub const INTERPRETATION_VERSION: &str = "typed_relationships_v1";
pub use model::{
    Edge, Graph, Kind, Subject, script_subject_id, transaction_subject_id, validate,
    validate_with_contract,
};
pub use query::{Neighborhood, Query, query_template, read_neighborhood};
