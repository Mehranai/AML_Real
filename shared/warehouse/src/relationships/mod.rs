//! Typed relationship interpretation and reads over the common storage schema.
//! Bitcoin input/output allocation is never inferred.
mod model;
mod page;
mod query;
pub const INTERPRETATION_VERSION: &str = "typed_relationships_v1";
pub use model::{
    Edge, Graph, Kind, Subject, script_subject_id, transaction_subject_id, validate,
    validate_with_contract,
};
pub use page::{PageError, PageQuery, TransferDirection, read_page};
pub use query::{Neighborhood, Query, query_template, read_neighborhood};
