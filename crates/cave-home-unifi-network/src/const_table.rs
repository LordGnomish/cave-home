// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/const.py
//
// Line-by-line port of HA UniFi integration constants.

/// Integration domain (HA: `DOMAIN = "unifi"`).
pub const DOMAIN: &str = "unifi";

/// Default site ID — every UniFi controller exposes a `default` site.
/// HA: `CONF_SITE_ID = "site"`. cave-home stores the site value, not the
/// HA-style config key name.
pub const DEFAULT_SITE: &str = "default";

/// Storage key for the wireless-clients persistence file (HA:
/// `STORAGE_KEY = "unifi_data"`).
pub const STORAGE_KEY: &str = "unifi_data";

/// Storage schema version (HA: `STORAGE_VERSION = 1`).
pub const STORAGE_VERSION: u32 = 1;

/// Default detection time in seconds before a wireless client is marked
/// "away" (HA: `DEFAULT_DETECTION_TIME = 300`).
pub const DEFAULT_DETECTION_TIME_SECS: u32 = 300;

/// HA: `DEFAULT_TRACK_CLIENTS = True`.
pub const DEFAULT_TRACK_CLIENTS: bool = true;

/// HA: `DEFAULT_TRACK_DEVICES = True`.
pub const DEFAULT_TRACK_DEVICES: bool = true;

/// HA: `DEFAULT_TRACK_WIRED_CLIENTS = True`.
pub const DEFAULT_TRACK_WIRED_CLIENTS: bool = true;

/// HA: `DEFAULT_ALLOW_BANDWIDTH_SENSORS = False`.
pub const DEFAULT_ALLOW_BANDWIDTH_SENSORS: bool = false;

/// HA: `DEFAULT_ALLOW_UPTIME_SENSORS = False`.
pub const DEFAULT_ALLOW_UPTIME_SENSORS: bool = false;

/// HA: `DEFAULT_DPI_RESTRICTIONS = True`.
pub const DEFAULT_DPI_RESTRICTIONS: bool = true;

/// HA: `DEFAULT_IGNORE_WIRED_BUG = False`.
pub const DEFAULT_IGNORE_WIRED_BUG: bool = false;

/// HA: `ATTR_MANUFACTURER = "Ubiquiti Networks"`.
pub const ATTR_MANUFACTURER: &str = "Ubiquiti Networks";

/// Switch kind discriminators (HA: `BLOCK_SWITCH = "block"`,
/// `DPI_SWITCH = "dpi"`, `OUTLET_SWITCH = "outlet"`).
pub const BLOCK_SWITCH_KIND: &str = "block";
pub const DPI_SWITCH_KIND: &str = "dpi";
pub const OUTLET_SWITCH_KIND: &str = "outlet";

/// HA: `Platform.SWITCH` / `Platform.SENSOR` / etc. The HA integration
/// declares 7 platforms; cave-home Phase 1 surfaces 5.
pub const PLATFORMS: &[&str] = &[
    "button",
    "device_tracker",
    "sensor",
    "switch",
    "update",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_unifi() {
        assert_eq!(DOMAIN, "unifi");
    }

    #[test]
    fn default_detection_time_300s() {
        assert_eq!(DEFAULT_DETECTION_TIME_SECS, 300);
    }

    #[test]
    fn manufacturer_is_ubiquiti_networks() {
        assert_eq!(ATTR_MANUFACTURER, "Ubiquiti Networks");
    }

    #[test]
    fn platforms_covers_phase1_surfaces() {
        assert!(PLATFORMS.contains(&"switch"));
        assert!(PLATFORMS.contains(&"sensor"));
        assert!(PLATFORMS.contains(&"device_tracker"));
    }
}
