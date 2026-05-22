// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! mDNS / DNS-SD advertisement payload for the Hue Bridge.
//!
//! Reference: developer-portal "Hue Bridge discovery" → mDNS section.
//! v2 bridges advertise `_hue._tcp.local.` with a TXT record carrying
//! `bridgeid=<bridge-id-uppercase>` and `modelid=<model-id>`. The instance
//! name is typically "Philips Hue - XXYYZZ" (last 6 hex chars of bridge-id).
//!
//! This module builds the **payload** that an mDNS responder transmits;
//! the actual UDP / multicast machinery sits behind a `MdnsResponder` trait
//! the cave-home binary supplies.

use crate::config::BridgeIdentity;

/// One DNS-SD service advertisement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsAdvertisement {
    /// Service type — always `_hue._tcp.local.` per docs.
    pub service_type: String,
    /// Friendly instance name — `Philips Hue - <last 6 hex of bridge-id uppercase>`.
    pub instance_name: String,
    /// Host name, e.g. "Hue-Bridge-XXYYZZ.local.".
    pub hostname: String,
    /// TCP port (443 for v2 CLIP).
    pub port: u16,
    /// TXT record key/value pairs.
    pub txt: Vec<(String, String)>,
}

/// Build the standard advertisement for our emulated bridge.
/// Reference: dev-portal mDNS docs + observed shipping-bridge mDNS records.
#[must_use]
pub fn build_advertisement(identity: &BridgeIdentity) -> MdnsAdvertisement {
    let bridge_id_upper = identity.bridge_id.to_uppercase();
    let suffix = bridge_id_upper
        .get(bridge_id_upper.len().saturating_sub(6)..)
        .unwrap_or(&bridge_id_upper);
    MdnsAdvertisement {
        service_type: "_hue._tcp.local.".into(),
        instance_name: format!("Philips Hue - {suffix}"),
        hostname: format!("Hue-Bridge-{suffix}.local."),
        port: identity.https_port,
        txt: vec![
            ("bridgeid".into(), bridge_id_upper),
            ("modelid".into(), identity.model_id.clone()),
        ],
    }
}

/// True iff the TXT record carries every key the developer-portal docs
/// require for a Hue Bridge advertisement.
#[must_use]
pub fn advertisement_is_compliant(ad: &MdnsAdvertisement) -> bool {
    let keys: Vec<&str> = ad.txt.iter().map(|(k, _)| k.as_str()).collect();
    ad.service_type == "_hue._tcp.local."
        && ad.port == 443
        && keys.contains(&"bridgeid")
        && keys.contains(&"modelid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_uses_documented_service_type_and_port() {
        let id = BridgeIdentity::fresh("10.0.0.7");
        let ad = build_advertisement(&id);
        assert_eq!(ad.service_type, "_hue._tcp.local.");
        assert_eq!(ad.port, 443);
        assert!(ad.instance_name.starts_with("Philips Hue - "));
        assert!(advertisement_is_compliant(&ad));
    }

    #[test]
    fn txt_record_carries_bridgeid_and_modelid() {
        let id = BridgeIdentity::fresh("10.0.0.7");
        let ad = build_advertisement(&id);
        let txt: std::collections::HashMap<_, _> = ad
            .txt
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(
            txt.get("bridgeid").unwrap(),
            &id.bridge_id.to_uppercase()
        );
        assert_eq!(txt.get("modelid").unwrap(), &id.model_id);
    }

    #[test]
    fn hostname_uses_last_six_hex_suffix() {
        let id = BridgeIdentity::fresh("10.0.0.7");
        let ad = build_advertisement(&id);
        let suffix = &id.bridge_id.to_uppercase()[6..];
        assert!(
            ad.hostname.contains(suffix),
            "hostname {} must include suffix {}",
            ad.hostname,
            suffix
        );
    }
}
