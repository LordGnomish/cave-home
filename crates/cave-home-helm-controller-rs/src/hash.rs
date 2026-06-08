// SPDX-License-Identifier: Apache-2.0
//! Deterministic content hash used for change detection.
//!
//! helm-controller decides whether to upgrade a release by comparing a hash of
//! the *desired* spec+values against the hash recorded on the last applied
//! release. Upstream stores this as an annotation derived from the rendered
//! job/values; we model the same idea with a small std-only FNV-1a hash over a
//! canonical serialization. The exact upstream digest algorithm is not the
//! point — *stability* (same input → same hash) and *sensitivity* (any
//! meaningful change → different hash) are. The wire-compatible annotation is
//! deferred to Phase 1b (see parity manifest).

/// 64-bit FNV-1a over UTF-8 bytes. Std-only, no allocation, deterministic
/// across runs and platforms.
#[must_use]
pub fn fnv1a64(input: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Render a hash as a short, stable lowercase-hex string (helm-controller uses
/// a short hex suffix on derived resource names; we mirror the shape).
#[must_use]
pub fn short_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector_empty_string() {
        // FNV-1a 64-bit of "" is the offset basis.
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn known_vector_a() {
        // FNV-1a 64-bit of "a" — published reference value.
        assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn stable_across_calls() {
        assert_eq!(fnv1a64("nginx:1.2.3|img=1"), fnv1a64("nginx:1.2.3|img=1"));
    }

    #[test]
    fn sensitive_to_change() {
        assert_ne!(fnv1a64("nginx:1.2.3"), fnv1a64("nginx:1.2.4"));
    }

    #[test]
    fn short_hex_is_16_chars() {
        assert_eq!(short_hex(fnv1a64("x")).len(), 16);
    }
}
