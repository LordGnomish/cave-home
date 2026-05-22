// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! Emulated bridge identity + configuration.
//!
//! Reference: developers.meethue.com/develop/hue-api/7-configuration-api/ and
//! developers.meethue.com/develop/application-design-guidance/using-https/
//! — the bridge serves its identity from three places:
//!
//! 1. `GET /description.xml` (UPnP SSDP) — model + serial + bridge-id.
//! 2. `GET /api/config` (anonymous) — short config: bridgeid, name, modelid,
//!    swversion, apiversion, mac, datastoreversion.
//! 3. `GET /api/<appkey>/config` (authenticated) — full config including
//!    portal services, whitelist, network info.

use serde::{Deserialize, Serialize};

/// Default Hue v2 bridge model identifier — BSB002.
/// Reference: Philips Hue bridge model registry (developer-portal).
pub const DEFAULT_MODEL_ID: &str = "BSB002";

/// Default v2 bridge product name. Reference: same registry.
pub const DEFAULT_PRODUCT_NAME: &str = "Philips hue";

/// Default v2 bridge manufacturer. Reference: Hue developer-portal docs.
pub const DEFAULT_MANUFACTURER: &str = "Signify Netherlands B.V.";

/// Default Hue v1 API version we emulate. We chose the latest documented
/// stable v1 minor at the time of writing.
pub const DEFAULT_API_VERSION_V1: &str = "1.66.0";

/// Identity carried by the emulator on the wire. Stable across restarts so
/// paired clients keep working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeIdentity {
    /// Friendly name. Default Turkish-friendly: "Cave Hue Köprüsü". Mapped
    /// in the Portal admin module per ADR-007.
    pub name: String,
    /// Normalised 12-character lowercase hex bridge ID (the upstream
    /// publishes this as the value used by NUPNP / mDNS / UPnP).
    pub bridge_id: String,
    /// MAC address shown by `GET /api/config.mac`.
    pub mac: String,
    /// Model identifier. Default [`DEFAULT_MODEL_ID`].
    pub model_id: String,
    /// Manufacturer string.
    pub manufacturer_name: String,
    /// Product name.
    pub product_name: String,
    /// Software version reported on the wire. Caller chooses; the official
    /// bridge uses a 10-digit numeric build number.
    pub software_version: String,
    /// `apiversion` string. Default [`DEFAULT_API_VERSION_V1`].
    pub api_version: String,
    /// Stable RFC 4122 UUID — used as the UPnP `<UDN>` and the v2
    /// bridge resource id.
    pub uuid: uuid::Uuid,
    /// Internal IP advertised by `/description.xml` + NUPNP.
    pub host: String,
    /// HTTP port (default 80 for v1, 443 for v2 / CLIP).
    pub http_port: u16,
    /// HTTPS port (default 443 for v2 CLIP).
    pub https_port: u16,
    /// Local datastore version — `/api/config.datastoreversion` ("100" on
    /// shipping bridges as of 1.66.0 docs).
    pub datastore_version: String,
}

impl BridgeIdentity {
    /// Build a fresh identity with random UUID + bridge-id, given a host.
    /// We compute a stable MAC by extracting the bridge-id middle.
    #[must_use]
    pub fn fresh(host: impl Into<String>) -> Self {
        let uuid = uuid::Uuid::new_v4();
        let mac = derive_mac_from_uuid(uuid);
        let bridge_id = derive_bridge_id_from_mac(&mac);
        Self {
            name: "Cave Hue Köprüsü".into(),
            bridge_id,
            mac,
            model_id: DEFAULT_MODEL_ID.into(),
            manufacturer_name: DEFAULT_MANUFACTURER.into(),
            product_name: DEFAULT_PRODUCT_NAME.into(),
            software_version: "1966076050".into(),
            api_version: DEFAULT_API_VERSION_V1.into(),
            uuid,
            host: host.into(),
            http_port: 80,
            https_port: 443,
            datastore_version: "100".into(),
        }
    }

    /// Render the SSDP `<UDN>` field as `uuid:2f402f80-da50-11e1-9b23-<mac>`.
    /// Reference: Hue developer-portal "Bridge discovery" — UPnP UDN format.
    #[must_use]
    pub fn ssdp_udn(&self) -> String {
        format!(
            "uuid:2f402f80-da50-11e1-9b23-{}",
            self.mac.replace(':', "")
        )
    }
}

/// Derive a 6-byte MAC string `XX:XX:XX:XX:XX:XX` from a UUID. Uses the
/// last 6 bytes of the UUID, OR-ing the locally-administered bit on so the
/// MAC can't collide with real hardware.
#[must_use]
pub fn derive_mac_from_uuid(uuid: uuid::Uuid) -> String {
    let bytes = uuid.as_bytes();
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&bytes[10..16]);
    mac[0] |= 0x02; // locally-administered bit
    mac[0] &= 0xfe; // unicast (clear multicast)
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

/// Derive the normalised 12-character bridge-id from a colon-separated MAC.
/// Reference: Hue developer-portal — "the bridge ID is the MAC address
/// without colons, lowercase".
#[must_use]
pub fn derive_bridge_id_from_mac(mac: &str) -> String {
    mac.replace(':', "").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_identity_has_consistent_mac_and_bridge_id() {
        let id = BridgeIdentity::fresh("10.0.0.5");
        assert_eq!(id.bridge_id.len(), 12);
        assert!(id.mac.len() == 17 && id.mac.matches(':').count() == 5);
        assert_eq!(id.bridge_id, id.mac.replace(':', "").to_lowercase());
        assert_eq!(id.host, "10.0.0.5");
    }

    #[test]
    fn locally_administered_bit_is_set() {
        let id = BridgeIdentity::fresh("10.0.0.1");
        let first_octet = u8::from_str_radix(&id.mac[0..2], 16).unwrap();
        assert!(first_octet & 0x02 != 0, "LAA bit must be set");
        assert!(first_octet & 0x01 == 0, "must be unicast");
    }

    #[test]
    fn ssdp_udn_matches_documented_format() {
        let id = BridgeIdentity::fresh("10.0.0.1");
        let udn = id.ssdp_udn();
        assert!(udn.starts_with("uuid:2f402f80-da50-11e1-9b23-"));
        let suffix = udn.trim_start_matches("uuid:2f402f80-da50-11e1-9b23-");
        assert_eq!(suffix.len(), 12); // bare MAC, no colons
    }

    #[test]
    fn defaults_match_documented_signify_bridge() {
        let id = BridgeIdentity::fresh("10.0.0.1");
        assert_eq!(id.model_id, "BSB002");
        assert_eq!(id.api_version, "1.66.0");
        assert_eq!(id.manufacturer_name, "Signify Netherlands B.V.");
    }
}
