use sha2::{Digest, Sha256};

/// Hex address to Base58Check. Accepts Tron 21-byte hex (`41...`),
/// EVM-style 20-byte hex, and padded ABI topics.
pub fn hex_to_base58(hex_addr: &str) -> Option<String> {
    let cleaned = hex_addr.trim_start_matches("0x").trim_start_matches("0X");

    let normalized = normalize_hex_payload(cleaned)?;
    let bytes = hex::decode(normalized).ok()?;

    if bytes.len() != 21 || bytes[0] != 0x41 {
        return None;
    }

    let mut payload = bytes;

    let hash1 = Sha256::digest(&payload);
    let hash2 = Sha256::digest(hash1);

    let checksum = &hash2[0..4];
    payload.extend_from_slice(checksum);

    Some(bs58::encode(payload).into_string())
}

fn normalize_hex_payload(cleaned: &str) -> Option<String> {
    if !cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }

    match cleaned.len() {
        42 if cleaned.starts_with("41") => Some(cleaned.to_string()),
        40 => Some(format!("41{}", cleaned)),
        len if len > 42 => {
            let last_42 = &cleaned[len - 42..];

            if last_42.starts_with("41") {
                Some(last_42.to_string())
            } else {
                Some(format!("41{}", &cleaned[len - 40..]))
            }
        }
        _ => None,
    }
}

/// Base58Check to Tron hex (`41...`).
pub fn base58_to_hex(addr: &str) -> Option<String> {
    let decoded = bs58::decode(addr).into_vec().ok()?;

    if decoded.len() != 25 {
        return None;
    }

    let raw = &decoded[..decoded.len() - 4];
    let checksum = &decoded[decoded.len() - 4..];

    if raw.first().copied() != Some(0x41) {
        return None;
    }

    let hash1 = Sha256::digest(raw);
    let hash2 = Sha256::digest(hash1);

    if &hash2[0..4] != checksum {
        return None;
    }

    Some(hex::encode(raw).to_uppercase())
}

/// Normalize an address to Base58Check.
pub fn normalize_tron_address(addr: &str) -> Option<String> {
    if addr.is_empty() {
        return None;
    }

    if addr.starts_with('T') {
        return base58_to_hex(addr).map(|_| addr.to_string());
    }

    hex_to_base58(addr)
}

#[cfg(test)]
mod tests {
    use super::{base58_to_hex, normalize_tron_address};

    #[test]
    fn normalizes_full_tron_hex() {
        let normalized = normalize_tron_address("4125ad4a9a23d1865faeaea080322f3e08cc205489")
            .expect("valid Tron hex");

        assert!(normalized.starts_with('T'));
        assert_eq!(
            base58_to_hex(&normalized).as_deref(),
            Some("4125AD4A9A23D1865FAEAEA080322F3E08CC205489")
        );
    }

    #[test]
    fn normalizes_padded_abi_topic_without_tron_prefix() {
        let normalized = normalize_tron_address(
            "00000000000000000000000030760c7e10b1d3509d8d64a7e9eb9ab94bc83495",
        )
        .expect("valid ABI topic");

        assert!(normalized.starts_with('T'));
        assert_eq!(
            base58_to_hex(&normalized).as_deref(),
            Some("4130760C7E10B1D3509D8D64A7E9EB9AB94BC83495")
        );
    }
}
