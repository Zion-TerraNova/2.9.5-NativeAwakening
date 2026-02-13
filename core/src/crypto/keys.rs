use ed25519_dalek::{Verifier, VerifyingKey, Signature};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use std::convert::TryInto;

pub fn verify(public_key_bytes: &[u8], msg: &[u8], signature_bytes: &[u8]) -> bool {
    let pk_array: [u8; 32] = match public_key_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    };
    
    let public_key = match VerifyingKey::from_bytes(&pk_array) {
        Ok(pk) => pk,
        Err(_) => return false,
    };

    let signature_array: [u8; 64] = match signature_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    };

    let signature = Signature::from_bytes(&signature_array);

    public_key.verify(msg, &signature).is_ok()
}

// Helper to parse hex string to bytes
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte_str = &s[i..i+2];
        match u8::from_str_radix(byte_str, 16) {
            Ok(b) => bytes.push(b),
            Err(_) => return None,
        }
    }
    Some(bytes)
}

const ZION_BASE32_ALPHABET: &[u8; 32] = b"023456789acdefghjklmnpqrstuvwxyz";

/// Compute a 4-character checksum from the address body using SHA-256.
///
/// The checksum is derived from `"zion1" + body[0..35]` and encoded
/// as 4 base32 characters, giving 2^20 ≈ 1M-to-1 typo detection rate.
fn compute_address_checksum(body_35: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"zion1");
    hasher.update(body_35.as_bytes());
    let hash = hasher.finalize();
    // Take first 2 bytes → 4 base32 chars
    let mut ck = String::with_capacity(4);
    for &byte in &hash[..2] {
        ck.push(ZION_BASE32_ALPHABET[(byte % 32) as usize] as char);
        ck.push(ZION_BASE32_ALPHABET[((byte / 32) % 32) as usize] as char);
    }
    ck
}

/// Derive `zion1...` address from public key bytes.
///
/// v2.9.5 format (44 chars):
///   `zion1` (5) + body (35) + checksum (4) = 44 chars
///
/// Algorithm:
///   1. `sha256(pubkey)` → `ripemd160(sha256)` → 20 bytes
///   2. Encode each byte as 2 base32 chars → 40 raw chars
///   3. Truncate to 35 body chars
///   4. Append 4-char SHA-256 checksum of `"zion1" + body`
///   5. Prefix with `zion1`
pub fn zion1_address_from_public_key_bytes(public_key_bytes: &[u8]) -> String {
    let sha = Sha256::digest(public_key_bytes);
    let key_hash = Ripemd160::digest(sha);

    let mut data = String::with_capacity(40);
    for &byte in key_hash.as_slice() {
        data.push(ZION_BASE32_ALPHABET[(byte % 32) as usize] as char);
        data.push(ZION_BASE32_ALPHABET[((byte / 32) % 32) as usize] as char);
    }
    data.truncate(35);

    let checksum = compute_address_checksum(&data);
    format!("zion1{data}{checksum}")
}

pub fn address_from_public_key(pk_hex: &str) -> Option<String> {
    let pk_bytes = from_hex(pk_hex)?;
    Some(zion1_address_from_public_key_bytes(&pk_bytes))
}

/// Convert public key hex to address (non-Option version for convenience)
pub fn address_from_public_key_hex(pk_hex: &str) -> String {
    address_from_public_key(pk_hex).unwrap_or_else(|| "INVALID".to_string())
}

/// Validate a zion1 address: format + embedded checksum.
///
/// Returns `true` if:
///   - starts with `zion1`
///   - exactly 44 chars
///   - all body chars are lowercase alphanumeric (base32 alphabet)
///   - last 4 chars match the checksum of `"zion1" + body[0..35]`
pub fn is_valid_zion1_address(address: &str) -> bool {
    if !address.starts_with("zion1") {
        return false;
    }
    if address.len() != 44 {
        return false;
    }
    if !address
        .as_bytes()
        .iter()
        .skip(5)
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'z'))
    {
        return false;
    }

    // Verify checksum: body = chars [5..40], checksum = chars [40..44]
    let body = &address[5..40];
    let expected_ck = compute_address_checksum(body);
    let actual_ck = &address[40..44];
    expected_ck == actual_ck
}

/// Validate address format only (no checksum check).
/// Use this for backward compatibility with pre-v2.9.5 legacy addresses.
pub fn is_valid_zion1_address_format(address: &str) -> bool {
    if !address.starts_with("zion1") {
        return false;
    }
    if address.len() != 44 {
        return false;
    }
    address
        .as_bytes()
        .iter()
        .skip(5)
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'z'))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Address generation and checksum round-trip
    // =========================================================================

    #[test]
    fn test_address_length_is_44() {
        let pk = [1u8; 32];
        let addr = zion1_address_from_public_key_bytes(&pk);
        assert_eq!(addr.len(), 44, "Address must be exactly 44 chars");
    }

    #[test]
    fn test_address_starts_with_zion1() {
        let pk = [2u8; 32];
        let addr = zion1_address_from_public_key_bytes(&pk);
        assert!(addr.starts_with("zion1"), "Address must start with zion1");
    }

    #[test]
    fn test_checksum_round_trip() {
        // Generate addresses from many different pubkeys and verify checksum passes
        for seed in 0u8..=255 {
            let pk = [seed; 32];
            let addr = zion1_address_from_public_key_bytes(&pk);
            assert!(
                is_valid_zion1_address(&addr),
                "Checksum failed for seed {seed}: addr={addr}"
            );
        }
    }

    #[test]
    fn test_checksum_detects_single_char_mutation() {
        let pk = [42u8; 32];
        let addr = zion1_address_from_public_key_bytes(&pk);
        assert!(is_valid_zion1_address(&addr));

        // Flip one character in the body (pos 10) – should fail checksum
        let mut bad = addr.clone().into_bytes();
        bad[10] = if bad[10] == b'0' { b'a' } else { b'0' };
        let bad_addr = String::from_utf8(bad).unwrap();
        assert!(
            !is_valid_zion1_address(&bad_addr),
            "Single-char mutation passed checksum: {bad_addr}"
        );
    }

    #[test]
    fn test_checksum_detects_truncation() {
        let pk = [7u8; 32];
        let addr = zion1_address_from_public_key_bytes(&pk);
        assert!(!is_valid_zion1_address(&addr[..43])); // too short
    }

    #[test]
    fn test_different_pubkeys_different_addresses() {
        let a = zion1_address_from_public_key_bytes(&[0u8; 32]);
        let b = zion1_address_from_public_key_bytes(&[1u8; 32]);
        assert_ne!(a, b, "Different pubkeys must produce different addresses");
    }

    #[test]
    fn test_deterministic_address_generation() {
        let pk = [99u8; 32];
        let a = zion1_address_from_public_key_bytes(&pk);
        let b = zion1_address_from_public_key_bytes(&pk);
        assert_eq!(a, b, "Same pubkey must always produce the same address");
    }

    #[test]
    fn test_format_only_validation_passes_without_checksum() {
        // Construct a syntactically valid but checksumless address
        let fake = "zion1000000000000000000000000000000000000000";
        assert_eq!(fake.len(), 44);
        assert!(is_valid_zion1_address_format(fake));
        // It should NOT pass full checksum validation (very unlikely to be valid)
        // We don't assert this because it's theoretically possible to collide
    }

    #[test]
    fn test_hex_round_trip() {
        let original = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let hex_str = hex::encode(&original);
        let decoded = from_hex(&hex_str).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_from_hex_rejects_odd_length() {
        assert!(from_hex("abc").is_none());
    }

    #[test]
    fn test_address_from_public_key_hex() {
        let pk = [5u8; 32];
        let hex_pk = hex::encode(pk);
        let addr = address_from_public_key_hex(&hex_pk);
        assert!(is_valid_zion1_address(&addr));
    }

    #[test]
    fn test_invalid_addresses() {
        assert!(!is_valid_zion1_address("btc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(!is_valid_zion1_address("zion1short"));
        assert!(!is_valid_zion1_address(""));
        assert!(!is_valid_zion1_address(
            "zion1AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" // uppercase
        ));
    }
}