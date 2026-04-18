//! Claim-Check threshold constants and small helpers.
//!
//! Frozen lisp §4.2.b claim-check:
//!   payload_size_hint() <= 8 KB → inline
//!   payload_size_hint()  > 8 KB → blob_storage + PayloadRef
//!
//! The threshold is a single source of truth consulted by `LogWriter` when
//! deciding which column to populate. Changing this value is a semantic
//! change that must be coordinated with downstream readers.

/// Inline vs ref split boundary. Strictly bytes (JSON-serialized length).
pub const CLAIM_CHECK_THRESHOLD: usize = 8192;

/// Compute the SHA-256 checksum of the provided bytes.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut fixed = [0u8; 32];
    fixed.copy_from_slice(&out);
    fixed
}

/// Hex-encode a 32-byte checksum — used by `PayloadRef` serialization and
/// by the `blob_storage.checksum` column (stored as `TEXT`).
pub fn hex_encode_checksum(checksum: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in checksum {
        write!(&mut s, "{:02x}", b).expect("write to String never fails");
    }
    s
}

/// Reverse of `hex_encode_checksum`. Returns `None` on malformed input.
pub fn hex_decode_checksum(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = char_to_nibble(chunk[0])?;
        let lo = char_to_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn char_to_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_8kb() {
        assert_eq!(CLAIM_CHECK_THRESHOLD, 8192);
    }

    #[test]
    fn sha256_deterministic() {
        let a = sha256(b"hello");
        let b = sha256(b"hello");
        assert_eq!(a, b);
        let c = sha256(b"world");
        assert_ne!(a, c);
    }

    #[test]
    fn checksum_hex_round_trip() {
        let ck = sha256(b"phase 2 storage");
        let hex = hex_encode_checksum(&ck);
        assert_eq!(hex.len(), 64);
        let decoded = hex_decode_checksum(&hex).expect("decode");
        assert_eq!(decoded, ck);
    }

    #[test]
    fn hex_decode_rejects_malformed() {
        assert!(hex_decode_checksum("not hex").is_none());
        assert!(hex_decode_checksum(&"z".repeat(64)).is_none());
    }
}
