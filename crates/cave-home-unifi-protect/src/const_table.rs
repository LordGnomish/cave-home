// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifiprotect/const.py

/// Integration domain (HA: `DOMAIN = "unifiprotect"`).
pub const DOMAIN: &str = "unifiprotect";

/// HA: `AUTH_RETRIES = 2` — 4.x+ returns 429 on rate-limit, so the
/// retry count is intentionally low.
pub const AUTH_RETRIES: u32 = 2;

/// HA: `DEFAULT_PORT = 443`.
pub const DEFAULT_PORT: u16 = 443;

/// HA: `DEFAULT_ATTRIBUTION = "Powered by UniFi Protect Server"`.
pub const DEFAULT_ATTRIBUTION: &str = "Powered by UniFi Protect Server";

/// HA: `DEFAULT_BRAND = "Ubiquiti"`.
pub const DEFAULT_BRAND: &str = "Ubiquiti";

/// HA: `DEFAULT_VERIFY_SSL = False`. UniFi NVRs ship with self-signed
/// certificates by default.
pub const DEFAULT_VERIFY_SSL: bool = false;

/// HA: `DEFAULT_MAX_MEDIA = 1000` (max media-source entries surfaced).
pub const DEFAULT_MAX_MEDIA: u32 = 1000;

/// Minimum supported UniFi Protect version (HA: `MIN_REQUIRED_PROTECT_V = Version("6.0.0")`).
pub const MIN_PROTECT_VERSION: &str = "6.0.0";

/// HA: `TYPE_EMPTY_VALUE = ""`. Sentinel for "no value".
pub const TYPE_EMPTY_VALUE: &str = "";

/// Event-type discriminators (HA: const.py final strings).
pub const EVENT_TYPE_FINGERPRINT_IDENTIFIED: &str = "identified";
pub const EVENT_TYPE_FINGERPRINT_NOT_IDENTIFIED: &str = "not_identified";
pub const EVENT_TYPE_NFC_SCANNED: &str = "scanned";
pub const EVENT_TYPE_VEHICLE_DETECTED: &str = "detected";

/// Delay in seconds before firing the vehicle event after the last
/// thumbnail (HA: `VEHICLE_EVENT_DELAY_SECONDS = 3`).
pub const VEHICLE_EVENT_DELAY_SECONDS: u32 = 3;

/// HA: `Platform.CAMERA`, `Platform.EVENT`, `Platform.SENSOR`, ... 14
/// platforms total. cave-home Phase 1 surfaces the four pillar
/// platforms; the rest are Phase 2 tickets.
pub const PLATFORMS: &[&str] = &["camera", "event", "sensor", "switch"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_unifiprotect() {
        assert_eq!(DOMAIN, "unifiprotect");
    }

    #[test]
    fn min_protect_v6() {
        assert_eq!(MIN_PROTECT_VERSION, "6.0.0");
    }

    #[test]
    fn default_attribution_is_protect() {
        assert!(DEFAULT_ATTRIBUTION.contains("UniFi Protect"));
    }

    #[test]
    fn vehicle_delay_3_seconds() {
        assert_eq!(VEHICLE_EVENT_DELAY_SECONDS, 3);
    }
}
