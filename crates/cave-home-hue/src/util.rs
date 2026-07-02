// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/util.py
//! Hue utility helpers. Ports `aiohue.util` line-by-line.
//!
//! The Python helper file mixes three concerns: (1) `create_app_key` for the
//! pairing flow, (2) bridge-ID normalisation, and (3) a runtime dataclass <-
//! dict reflection layer used by aiohue's v2 controllers. Rust uses serde
//! attributes instead of runtime reflection, so we port (1) and (2) here.
//! The dataclass reflection layer of `aiohue.util` is replaced by `serde` in
//! `crate::v2::models` (see ADR-010 — the surface preserves the upstream
//! shape; the parse engine differs because Python lacks `serde`).
//!
//! `create_app_key` lives in [`crate::bridge`] alongside the high-level
//! pairing flow, not here — see `aiohue.util.create_app_key` for the upstream
//! reference.

/// Normalise a Hue bridge identifier.
///
/// Source: `aiohue.util.normalize_bridge_id`. The bridge ID arrives in three
/// shapes:
///
/// 1. zeroconf / mDNS — `properties['id']` carries a colon-separated MAC-like
///    17-character form. We strip the colons.
/// 2. NUPNP — the bridge ID includes 4 "fffe" filler characters in the middle
///    of a 16-character string. We splice them out.
/// 3. SSDP / Hue API — already a 12-character lowercase hex string.
///
/// Anything that doesn't match warns and passes through.
#[must_use]
pub fn normalize_bridge_id(bridge_id: &str) -> String {
    let bridge_id = bridge_id.to_lowercase();

    // zeroconf: colon-separated MAC, 17 chars (5 colons)
    if bridge_id.len() == 17 && bridge_id.matches(':').count() == 5 {
        return bridge_id.replace(':', "");
    }

    // nupnp: 16 chars with "fffe" filler at offset 6..10
    if bridge_id.len() == 16 && bridge_id.get(6..10) == Some("fffe") {
        let head = bridge_id.get(0..6).unwrap_or("");
        let tail = bridge_id.get(10..16).unwrap_or("");
        return format!("{head}{tail}");
    }

    // SSDP / API form: 12 lowercase hex chars.
    if bridge_id.len() == 12 {
        return bridge_id;
    }

    // unexpected — log and pass through, matching upstream behaviour.
    tracing::warn!(received = %bridge_id, "Received unexpected bridge id");
    bridge_id
}

/// Parse a MAC address from a normalised bridge ID.
///
/// Source: `aiohue.util.mac_from_bridge_id`. The 16-char NUPNP form embeds a
/// MAC-48 address with "fffe" in the middle as a 64-bit EUI-64 extension.
/// We assume the input has already been normalised to 12 chars.
///
/// Returns `None` if the input is not 12 hex characters.
#[must_use]
pub fn mac_from_bridge_id(bridge_id: &str) -> Option<String> {
    if bridge_id.len() != 12 || !bridge_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    // Mirror upstream slicing exactly.
    let parts = [
        &bridge_id[0..2],
        &bridge_id[2..4],
        &bridge_id[4..6],
        &bridge_id[6..8],
        &bridge_id[8..10],
        &bridge_id[10..12],
    ];
    Some(parts.join(":"))
}

/// Format a `datetime`-style UTC timestamp the way Hue v1 wants it.
///
/// Source: `aiohue.util.format_utc_timestamp` — Python format string
/// `"%Y-%m-%dT%H:%M:%S.%fZ"`. We emit a lightweight `&str` formatter so
/// callers don't pull in `chrono` just for one helper.
#[must_use]
pub fn format_utc_timestamp_components(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    micros: u32,
) -> String {
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeroconf_id_is_stripped() {
        // Upstream contract: 17-char colon-separated form (MAC-48) becomes
        // 12-char lowercase hex string.
        let out = normalize_bridge_id("00:17:88:AB:CD:EF");
        assert_eq!(out, "001788abcdef");
    }

    #[test]
    fn nupnp_id_filler_is_removed() {
        let out = normalize_bridge_id("001788FFFE001234");
        assert_eq!(out, "001788001234");
    }

    #[test]
    fn ssdp_id_passes_through_lowercased() {
        let out = normalize_bridge_id("001788ABCDEF");
        assert_eq!(out, "001788abcdef");
    }

    #[test]
    fn unknown_form_is_passed_through() {
        let out = normalize_bridge_id("definitely-not-a-hue-id");
        // Upstream warns + returns the original (lowercased).
        assert_eq!(out, "definitely-not-a-hue-id");
    }

    #[test]
    fn mac_from_bridge_id_splits_into_colon_groups() {
        let mac = mac_from_bridge_id("001788abcdef").expect("valid 12-char id");
        assert_eq!(mac, "00:17:88:ab:cd:ef");
    }

    #[test]
    fn mac_from_bridge_id_rejects_wrong_length() {
        assert!(mac_from_bridge_id("001788").is_none());
        assert!(mac_from_bridge_id("xxxxxxxxxxxx").is_none());
    }

    #[test]
    fn format_utc_timestamp_components_matches_hue_format() {
        let out = format_utc_timestamp_components(2026, 5, 17, 12, 30, 45, 123_456);
        assert_eq!(out, "2026-05-17T12:30:45.123456Z");
    }
}
