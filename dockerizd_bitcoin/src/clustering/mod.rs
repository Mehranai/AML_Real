//! Bounded investigation leads, never an entity registry or risk seed.
pub mod cli;
mod detect;
mod store;
#[cfg(test)]
mod tests;
use serde_json::{Value, json};
pub use store::{Conflict, Review, compute, inspect, review, snapshot, validate_id};
pub const VERSION: &str = "bitcoin_cluster_leads_v1";
pub(crate) const MAX_ROOTS: usize = 25;
pub(crate) const MAX_MEMBERS: usize = 16;
pub(crate) fn digest(value: &Value) -> String {
    aml_risk_policy::digest(value)
}
pub fn config() -> Value {
    let definition = json!({"detector_version":VERSION,
        "common_input":{"minimum_distinct_addresses":2,"maximum_distinct_addresses":MAX_MEMBERS,"confidence":0.5,"transitive_union":false},
        "change":{"single_input":true,"positive_outputs":2,"unique_matching_address_encoding_family":true,"both_outputs_different_from_input":true,"confidence":0.3},
        "collaboration_guard":{"minimum_distinct_input_addresses":2,"equal_positive_outputs_to_distinct_addresses":2,"action":"suppress_leads"},
        "limits":{"roots":MAX_ROOTS,"legs_per_transaction":256,"total_legs":4096,"evidence_bytes":512*1024},
        "ownership_claimed":false,"score_eligible":false,"automatic_approval":false,
        "limitations":["These project heuristics are investigative leads, not ownership or crime probabilities.","The collaboration guard does not identify every CoinJoin or PayJoin; every lead remains uncertain.","A change candidate is a narrow address-encoding-family heuristic, not a proven change output.","Review accepts or rejects a lead only; it cannot create an entity, seed, transfer allocation or risk signal.","There is no transitive address merging and no whole-chain clustering claim."]});
    json!({"contract":"bitcoin_clustering_config_v1","network_id":crate::NETWORK_ID,"detector_version":VERSION,"definition_hash":digest(&definition),"definition":definition})
}
