// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v1/config.py
//! v1 bridge config. Ports `aiohue.v1.config` line-by-line.
//!
//! Reference: <https://developers.meethue.com/documentation/configuration-api#72_get_configuration>.

use crate::errors::HueResult;
use crate::v1::api::V1Request;
use serde_json::Value;

/// Holds the bridge's `/config` payload. Source: `aiohue.v1.config.Config`.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub raw: serde_json::Map<String, Value>,
}

impl Config {
    /// Wrap an already-fetched payload. The upstream constructor takes
    /// `(raw, request)`; we keep the request out (callers pass it to `update`).
    #[must_use]
    pub fn from_raw(raw: serde_json::Map<String, Value>) -> Self {
        Self { raw }
    }

    /// `aiohue.v1.config.Config.bridge_id`.
    #[must_use]
    pub fn bridge_id(&self) -> Option<&str> {
        self.raw.get("bridgeid").and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.name`.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.raw.get("name").and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.mac_address`.
    #[must_use]
    pub fn mac_address(&self) -> Option<&str> {
        self.raw.get("mac").and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.model_id`.
    #[must_use]
    pub fn model_id(&self) -> Option<&str> {
        self.raw.get("modelid").and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.software_version`.
    #[must_use]
    pub fn software_version(&self) -> Option<&str> {
        self.raw.get("swversion").and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.swupdate2_bridge_state`.
    #[must_use]
    pub fn swupdate2_bridge_state(&self) -> Option<&str> {
        self.raw
            .get("swupdate2")
            .and_then(Value::as_object)
            .and_then(|m| m.get("bridge"))
            .and_then(Value::as_object)
            .and_then(|m| m.get("state"))
            .and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.apiversion`.
    #[must_use]
    pub fn api_version(&self) -> Option<&str> {
        self.raw.get("apiversion").and_then(Value::as_str)
    }

    /// `aiohue.v1.config.Config.update`.
    pub async fn update(&mut self, req: &dyn V1Request) -> HueResult<()> {
        let val = req.get("config").await?;
        if let Value::Object(m) = val {
            self.raw = m;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_exposes_bridge_metadata() {
        let raw = json!({
            "bridgeid": "001788FFFEABCDEF",
            "name": "Cave Hue Bridge",
            "mac": "00:17:88:ab:cd:ef",
            "modelid": "BSB002",
            "swversion": "1958076050",
            "apiversion": "1.66.0",
            "swupdate2": {"bridge": {"state": "noupdates"}}
        });
        let cfg = Config::from_raw(raw.as_object().unwrap().clone());
        assert_eq!(cfg.bridge_id(), Some("001788FFFEABCDEF"));
        assert_eq!(cfg.name(), Some("Cave Hue Bridge"));
        assert_eq!(cfg.mac_address(), Some("00:17:88:ab:cd:ef"));
        assert_eq!(cfg.model_id(), Some("BSB002"));
        assert_eq!(cfg.software_version(), Some("1958076050"));
        assert_eq!(cfg.api_version(), Some("1.66.0"));
        assert_eq!(cfg.swupdate2_bridge_state(), Some("noupdates"));
    }
}
