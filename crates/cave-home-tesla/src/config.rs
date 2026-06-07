// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The node configuration model for the Tesla energy adapter.
//!
//! This is the deserializable shape the single binary's layered config feeds in.
//! It ships **placeholder** credentials only — Burak's real client id / token
//! never live in the repo (Charter operational rule); the operational layer
//! fills them from the environment or the `0600` credential file.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_with_placeholders() {
        let c = TeslaConfig::default();
        assert!(!c.enabled);
        assert!(c.client_id.contains("REPLACE"));
        assert_eq!(c.region, "na");
        assert_eq!(c.rate_limit_secs, 30);
    }

    #[test]
    fn default_region_parses() {
        assert_eq!(TeslaConfig::default().region(), Some(Region::NorthAmericaAsiaPacific));
    }

    #[test]
    fn disabled_config_always_validates() {
        let c = TeslaConfig::default();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn enabled_requires_site_id() {
        let mut c = TeslaConfig::default();
        c.enabled = true;
        c.client_id = "real-client".into();
        assert!(c.validate().is_err());
        c.site_id = Some(123);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unknown_region() {
        let mut c = TeslaConfig::default();
        c.region = "mars".into();
        assert!(c.validate().is_err());
        assert_eq!(c.region(), None);
    }

    #[test]
    fn deserialises_from_json() {
        let json = r#"{
            "enabled": true,
            "region": "eu",
            "site_id": 987654321,
            "client_id": "cave-home-energy",
            "redirect_uri": "https://localhost:8443/callback",
            "rate_limit_secs": 30
        }"#;
        let c: TeslaConfig = serde_json::from_str(json).unwrap();
        assert!(c.enabled);
        assert_eq!(c.site_id, Some(987_654_321));
        assert_eq!(c.region(), Some(Region::Europe));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn gateway_config_optional() {
        let c = TeslaConfig::default();
        assert!(c.gateway.is_none());
        let json = r#"{"gateway":{"host":"https://192.168.1.10"}}"#;
        let c: TeslaConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.gateway.unwrap().host, "https://192.168.1.10");
    }
}
