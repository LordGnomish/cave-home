// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/
//
// The HA integration keys clients + devices by their MAC address (the
// `mac` field on `aiounifi.models.client.Client` and
// `aiounifi.models.device.Device`). cave-home wraps that in a
// distinct typed identifier per surface so they can't be mixed up at
// the API boundary.

use serde::{Deserialize, Serialize};

/// MAC-address-backed client identifier (HA: `client.mac`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(String);

impl ClientId {
    /// Create from any string. MAC addresses are normalised to
    /// lower-hex on construction so look-ups are case-insensitive (the
    /// UniFi controller returns mixed case across endpoints).
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into().to_lowercase())
    }

    /// Borrow the normalised string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// MAC-address-backed UniFi device identifier (HA: `device.mac`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(String);

impl DeviceId {
    /// Create from any string; MAC normalised to lower-hex.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into().to_lowercase())
    }

    /// Borrow the normalised string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// UniFi site identifier (HA: `CONF_SITE_ID = "site"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SiteId(String);

impl SiteId {
    /// Create from string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }

    /// Borrow the site name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SiteId {
    fn default() -> Self {
        Self::new(crate::const_table::DEFAULT_SITE)
    }
}

impl std::fmt::Display for SiteId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_lowercase() {
        assert_eq!(ClientId::new("AA:BB").as_str(), "aa:bb");
    }

    #[test]
    fn device_id_lowercase() {
        assert_eq!(DeviceId::new("AA:BB").as_str(), "aa:bb");
    }

    #[test]
    fn site_id_default_is_default() {
        assert_eq!(SiteId::default().as_str(), "default");
    }

    #[test]
    fn site_id_custom() {
        assert_eq!(SiteId::new("guest").as_str(), "guest");
    }
}
