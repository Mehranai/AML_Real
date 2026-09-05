use super::registry::ProtocolRegistry;
use super::types::ProtocolInfo;

pub fn detect_protocol(registry: &ProtocolRegistry, address: &str) -> Option<ProtocolInfo> {
    registry.lookup(address)
}
