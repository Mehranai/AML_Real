pub mod api;
pub(crate) mod graph;
mod wallet;
pub use graph::{Edge, PathOptions, SearchOptions, graph, paths};
pub use wallet::{investigate, status};

pub const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
