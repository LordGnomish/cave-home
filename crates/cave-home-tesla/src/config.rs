// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The node configuration model for the Tesla energy adapter.
//!
//! This is the deserializable shape the single binary's layered config feeds in.
//! It ships **placeholder** credentials only — Burak's real client id / token
//! never live in the repo (Charter operational rule); the operational layer
//! fills them from the environment or the `0600` credential file.

use serde::{Deserialize, Serialize};

use crate::error::{Result, TeslaError};
use crate::fleet_api::Region;
use crate::token_store::Secret;

/// The placeholder client id shipped in the repo. Replaced operationally.
const PLACEHOLDER_CLIENT_ID: &str = "REPLACE_WITH_TESLA_CLIENT_ID";

/// The Powerwall local-gateway settings (optional LAN fallback).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// The gateway base URL, e.g. `https://192.168.1.10`.
    pub host: String,
    /// The gateway login password (the operational layer fills this).
    #[serde(default)]
    pub password: Option<Secret>,
}

/// The Tesla energy adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeslaConfig {
    /// Whether the adapter is active.
    #[serde(default)]
    pub enabled: bool,
    /// The Fleet API region key (`na` / `eu` / `cn`).
    #[serde(default = "default_region")]
    pub region: String,
    /// The energy site id.
    #[serde(default)]
    pub site_id: Option<u64>,
    /// The registered OAuth client id.
    #[serde(default = "default_client_id")]
    pub client_id: String,
    /// The registered redirect URI.
    #[serde(default = "default_redirect_uri")]
    pub redirect_uri: String,
    /// The client secret, for confidential clients.
    #[serde(default)]
    pub client_secret: Option<Secret>,
    /// The credential-file path override (defaults to `~/.cave-home`).
    #[serde(default)]
    pub credentials_path: Option<String>,
    /// The per-endpoint rate-limit interval, seconds.
    #[serde(default = "default_rate_limit")]
    pub rate_limit_secs: u64,
    /// Optional Powerwall local-gateway settings.
    #[serde(default)]
    pub gateway: Option<GatewayConfig>,
}

fn default_region() -> String {
    "na".to_string()
}
fn default_client_id() -> String {
    PLACEHOLDER_CLIENT_ID.to_string()
}
fn default_redirect_uri() -> String {
    "https://localhost:8443/callback".to_string()
}
const fn default_rate_limit() -> u64 {
    30
}

impl Default for TeslaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            region: default_region(),
            site_id: None,
            client_id: default_client_id(),
            redirect_uri: default_redirect_uri(),
            client_secret: None,
            credentials_path: None,
            rate_limit_secs: default_rate_limit(),
            gateway: None,
        }
    }
}

impl TeslaConfig {
    /// The parsed [`Region`], or `None` if the key is unrecognised.
    #[must_use]
    pub fn region(&self) -> Option<Region> {
        Region::from_key(&self.region)
    }

    /// Validate the configuration.
    ///
    /// # Errors
    /// [`TeslaError::Validation`] for an unknown region, or — when enabled — a
    /// placeholder client id, an empty redirect URI or a missing site id.
    pub fn validate(&self) -> Result<()> {
        if self.region().is_none() {
            return Err(TeslaError::Validation(format!(
                "unknown Tesla region '{}' (expected na/eu/cn)",
                self.region
            )));
        }
        if !self.enabled {
            return Ok(());
        }
        if self.client_id.is_empty() || self.client_id == PLACEHOLDER_CLIENT_ID {
            return Err(TeslaError::Validation(
                "Tesla client_id is unset (placeholder)".into(),
            ));
        }
        if self.redirect_uri.is_empty() {
            return Err(TeslaError::Validation("Tesla redirect_uri is empty".into()));
        }
        if self.site_id.is_none() {
            return Err(TeslaError::Validation(
                "Tesla site_id is required when enabled".into(),
            ));
        }
        Ok(())
    }
}

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
