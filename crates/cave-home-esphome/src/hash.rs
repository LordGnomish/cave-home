// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `ESPHome` FNV-1 entity-key hash.

/// 32-bit FNV-1 hash — `ESPHome`'s `fnv1_hash` (esphome/core/helpers.cpp), which
/// derives an entity's stable native-API `key` from its `object_id`.
///
/// This is FNV-1, **not** FNV-1a: each byte is folded by multiplying first,
/// then xoring (`hash = (hash * prime) ^ byte`), starting from the 32-bit
/// offset basis. `ESPHome` object ids are ASCII (`[a-z0-9_]`), so hashing the
/// UTF-8 bytes reproduces the device-side value exactly.
#[must_use]
pub fn fnv1_hash(s: &str) -> u32 {
    const OFFSET_BASIS: u32 = 0x811C_9DC5; // 2166136261
    const PRIME: u32 = 0x0100_0193; // 16777619
    let mut hash = OFFSET_BASIS;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(PRIME);
        hash ^= u32::from(byte);
    }
    hash
}
