// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi_access/const.py

/// Integration domain (HA: `DOMAIN = "unifi_access"`).
pub const DOMAIN: &str = "unifi_access";

/// HA: `DEFAULT_LOCK_RULE_INTERVAL = 10`.
pub const DEFAULT_LOCK_RULE_INTERVAL: u32 = 10;

/// HA: `MAX_LOCK_RULE_INTERVAL = 480`.
pub const MAX_LOCK_RULE_INTERVAL: u32 = 480;

/// HA: `MIN_LOCK_RULE_INTERVAL = 1`.
pub const MIN_LOCK_RULE_INTERVAL: u32 = 1;

/// HA: `SERVICE_SET_LOCK_RULE = "set_lock_rule"`.
pub const SERVICE_SET_LOCK_RULE: &str = "set_lock_rule";

/// HA: `ATTR_INTERVAL = "interval"`.
pub const ATTR_INTERVAL: &str = "interval";

/// HA: `ATTR_RULE = "rule"`.
pub const ATTR_RULE: &str = "rule";

/// HA: 7 platforms total — binary_sensor, button, event, image,
/// select, sensor, switch.
pub const PLATFORMS: &[&str] = &[
    "binary_sensor",
    "button",
    "event",
    "image",
    "select",
    "sensor",
    "switch",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_unifi_access() {
        assert_eq!(DOMAIN, "unifi_access");
    }

    #[test]
    fn interval_bounds() {
        assert_eq!(MIN_LOCK_RULE_INTERVAL, 1);
        assert_eq!(MAX_LOCK_RULE_INTERVAL, 480);
        assert_eq!(DEFAULT_LOCK_RULE_INTERVAL, 10);
    }
}
