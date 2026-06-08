//! Discovery: turn "something appeared on the network" into "offer the
//! household a new device", without re-offering things they already added.
//!
//! The actual transports (mDNS / SSDP / DHCP listeners) are network-bound and
//! deferred to Phase 1b; they all boil down to a [`Discovered`] signal — a
//! transport, a service key and a bag of properties — which this module then
//! matches against the [`crate::integration::Registry`] and dedupes against the
//! config entries a household already has.

use crate::config_entry::ConfigEntry;
use crate::integration::Registry;

/// Which kind of network listener saw the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Multicast DNS / Bonjour (`_hue._tcp`, `_esphomelib._tcp`, …).
    Mdns,
    /// SSDP / `UPnP`.
    Ssdp,
    /// A DHCP lease (matched by MAC prefix / hostname).
    Dhcp,
}

/// A single discovery signal: a transport, a service key, and properties.
///
/// The `key` is whatever the transport uses to name the service class — an
/// mDNS service type, an SSDP device type, a DHCP hostname pattern. Properties
/// carry the per-device detail (`id`, `mac`, `serial`) used to dedupe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    transport: Transport,
    key: String,
    properties: Vec<(String, String)>,
}

impl Discovered {
    /// A signal from a given transport with a service key.
    #[must_use]
    pub fn new(transport: Transport, key: impl Into<String>) -> Self {
        Self { transport, key: key.into(), properties: Vec::new() }
    }

    /// Shorthand for an mDNS signal.
    #[must_use]
    pub fn mdns(key: impl Into<String>) -> Self {
        Self::new(Transport::Mdns, key)
    }

    /// Shorthand for an SSDP signal.
    #[must_use]
    pub fn ssdp(key: impl Into<String>) -> Self {
        Self::new(Transport::Ssdp, key)
    }

    /// Shorthand for a DHCP signal.
    #[must_use]
    pub fn dhcp(key: impl Into<String>) -> Self {
        Self::new(Transport::Dhcp, key)
    }

    /// Attach a property (e.g. the device's stable id).
    #[must_use]
    pub fn with_property(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.properties.push((k.into(), v.into()));
        self
    }

    #[must_use]
    pub const fn transport(&self) -> Transport {
        self.transport
    }

    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Read back a property value.
    #[must_use]
    pub fn property(&self, k: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(pk, _)| pk == k)
            .map(|(_, v)| v.as_str())
    }

    /// Derive the stable unique-id for this discovery, used for dedupe.
    ///
    /// Prefers an explicit stable property (`id`, then `serial`, then `mac`);
    /// falls back to the service key so a keyed-but-id-less device still
    /// dedupes against itself. The domain is prefixed so the same physical id
    /// reported for two integration kinds does not collide.
    #[must_use]
    pub fn unique_id(&self, domain: &str) -> String {
        let tail = self
            .property("id")
            .or_else(|| self.property("serial"))
            .or_else(|| self.property("mac"))
            .unwrap_or(&self.key);
        format!("{domain}:{tail}")
    }
}

/// One actionable result of matching a discovery signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The integration domain that can handle it.
    pub domain: String,
    /// The unique-id this device would get.
    pub unique_id: String,
    /// Whether the household already added this exact device.
    pub already_configured: bool,
}

/// Match a discovery signal against the registry, deduped against existing
/// config entries.
///
/// Returns one [`Candidate`] per integration that can handle the signal, each
/// flagged with whether the household already added that exact device (so the
/// UI can suppress the ones marked `already_configured`).
#[must_use]
pub fn candidates(
    registry: &Registry,
    entries: &[ConfigEntry],
    found: &Discovered,
) -> Vec<Candidate> {
    registry
        .match_discovery(found)
        .into_iter()
        .map(|domain| {
            let unique_id = found.unique_id(&domain);
            let already_configured = entries
                .iter()
                .any(|e| e.domain() == domain && e.unique_id() == Some(unique_id.as_str()));
            Candidate { domain, unique_id, already_configured }
        })
        .collect()
}

/// The subset of [`candidates`] the household has *not* already added — what
/// the "found a new device" UI should actually offer.
#[must_use]
pub fn new_candidates(
    registry: &Registry,
    entries: &[ConfigEntry],
    found: &Discovered,
) -> Vec<Candidate> {
    candidates(registry, entries, found)
        .into_iter()
        .filter(|c| !c.already_configured)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::integration::{Integration, IotClass};

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.register(
            Integration::new("hue", "Philips Hue")
                .with_capability(Capability::Light)
                .with_iot_class(IotClass::LocalPush)
                .discoverable_by("_hue._tcp"),
        );
        r.register(
            Integration::new("esphome", "ESPHome device")
                .with_capability(Capability::Sensor)
                .discoverable_by("_esphomelib._tcp"),
        );
        r
    }

    #[test]
    fn matches_only_the_handling_integration() {
        let r = registry();
        let found = Discovered::mdns("_hue._tcp").with_property("id", "AA11");
        assert_eq!(r.match_discovery(&found), vec!["hue".to_string()]);
    }

    #[test]
    fn unknown_service_key_matches_nothing() {
        let r = registry();
        let found = Discovered::mdns("_printer._tcp");
        assert!(r.match_discovery(&found).is_empty());
    }

    #[test]
    fn unique_id_prefers_id_then_serial_then_mac_then_key() {
        let by_id = Discovered::mdns("_hue._tcp").with_property("id", "X1");
        assert_eq!(by_id.unique_id("hue"), "hue:X1");
        let by_serial = Discovered::mdns("_hue._tcp").with_property("serial", "S9");
        assert_eq!(by_serial.unique_id("hue"), "hue:S9");
        let by_mac = Discovered::mdns("_hue._tcp").with_property("mac", "ab:cd");
        assert_eq!(by_mac.unique_id("hue"), "hue:ab:cd");
        let by_key = Discovered::mdns("_hue._tcp");
        assert_eq!(by_key.unique_id("hue"), "hue:_hue._tcp");
    }

    #[test]
    fn new_device_is_offered() {
        let r = registry();
        let found = Discovered::mdns("_hue._tcp").with_property("id", "AA11");
        let news = new_candidates(&r, &[], &found);
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].domain, "hue");
        assert_eq!(news[0].unique_id, "hue:AA11");
        assert!(!news[0].already_configured);
    }

    #[test]
    fn already_added_device_is_not_offered_again() {
        let r = registry();
        let found = Discovered::mdns("_hue._tcp").with_property("id", "AA11");
        let existing = ConfigEntry::new("hue", "Hue Bridge").with_unique_id("hue:AA11");
        let all = candidates(&r, std::slice::from_ref(&existing), &found);
        assert!(all[0].already_configured);
        let news = new_candidates(&r, std::slice::from_ref(&existing), &found);
        assert!(news.is_empty(), "an already-added device must not be re-offered");
    }

    #[test]
    fn a_different_unit_of_the_same_kind_is_still_new() {
        let r = registry();
        let existing = ConfigEntry::new("hue", "Hue Bridge").with_unique_id("hue:AA11");
        let other = Discovered::mdns("_hue._tcp").with_property("id", "BB22");
        let news = new_candidates(&r, std::slice::from_ref(&existing), &other);
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].unique_id, "hue:BB22");
    }

    #[test]
    fn property_readback() {
        let found = Discovered::dhcp("espressif").with_property("mac", "24:6f:28");
        assert_eq!(found.property("mac"), Some("24:6f:28"));
        assert_eq!(found.property("missing"), None);
        assert_eq!(found.transport(), Transport::Dhcp);
    }
}
