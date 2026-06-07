// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Data-transfer objects for the SysAP REST JSON responses.
//!
//! The SysAP keys its responses by the System Access Point UUID, so both the
//! configuration and the device-list arrive wrapped in a one-entry map. These
//! DTOs deserialise that shape and ignore the parts cave-home does not model
//! (floorplan, users, …). Domain meaning (function → device kind, value codec)
//! is left to [`cave_home_free_home`]; here we only carry the raw fields.

use std::collections::BTreeMap;

use cave_home_free_home::Function;
use serde::Deserialize;

use crate::error::Result;

/// One datapoint as it appears in the configuration tree.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DatapointDto {
    /// The last reported wire value, if present.
    #[serde(default)]
    pub value: Option<String>,
}

/// One channel: a function plus its datapoints.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ChannelDto {
    /// The free@home function ID as a hex string (e.g. `"0012"`).
    #[serde(rename = "functionID", default)]
    pub function_id: Option<String>,
    /// Human display name, if the SysAP supplies one.
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    /// Datapoints keyed by id (`odp0000`, `idp0001`, …).
    #[serde(default)]
    pub datapoints: BTreeMap<String, DatapointDto>,
}

impl ChannelDto {
    /// The resolved free@home [`Function`], if the function ID is known.
    pub fn function(&self) -> Option<Function> {
        self.function_id
            .as_deref()
            .and_then(parse_function_id)
            .and_then(Function::from_id)
    }
}

/// One device: a display name plus its channels.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DeviceDto {
    /// Human display name, if the SysAP supplies one.
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    /// Channels keyed by id (`ch0000`, …).
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelDto>,
}

/// The configuration of a single System Access Point.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct SysApConfig {
    /// Devices keyed by serial.
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceDto>,
}

/// The `GET configuration` response: SysAP UUID → its configuration.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ConfigurationResponse(pub BTreeMap<String, SysApConfig>);

impl ConfigurationResponse {
    /// Deserialise from a JSON body.
    pub fn parse(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// The number of System Access Points in the response.
    pub fn sysap_count(&self) -> usize {
        self.0.len()
    }

    /// The first (typically only) SysAP entry, as `(uuid, config)`.
    pub fn first_sysap(&self) -> Option<(&str, &SysApConfig)> {
        self.0.iter().next().map(|(k, v)| (k.as_str(), v))
    }
}

/// The `GET devicelist` response: SysAP UUID → serials.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct DeviceListResponse(pub BTreeMap<String, Vec<String>>);

impl DeviceListResponse {
    /// Deserialise from a JSON body.
    pub fn parse(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Every device serial across all SysAPs, in deterministic order.
    pub fn serials(&self) -> Vec<&str> {
        self.0
            .values()
            .flat_map(|v| v.iter().map(String::as_str))
            .collect()
    }
}

/// Parse a free@home function/pairing hex string (`"0012"` or `"0x0012"`).
pub fn parse_function_id(raw: &str) -> Option<u16> {
    let hex = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    u16::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::Function;

    const CONFIG_JSON: &str = r#"{
      "00000000-0000-0000-0000-000000000000": {
        "devices": {
          "ABB700C12345": {
            "displayName": "Living Room Light",
            "channels": {
              "ch0000": {
                "functionID": "0012",
                "displayName": "Dimmer",
                "datapoints": {
                  "odp0000": { "value": "1" },
                  "odp0001": { "value": "80" }
                }
              }
            }
          }
        },
        "floorplan": { "ignored": true }
      }
    }"#;

    const DEVICELIST_JSON: &str =
        r#"{ "00000000-0000-0000-0000-000000000000": ["ABB700C12345", "ABB700C99999"] }"#;

    #[test]
    fn parse_devicelist_serials() {
        let dl = DeviceListResponse::parse(DEVICELIST_JSON).expect("parse");
        assert_eq!(dl.serials(), vec!["ABB700C12345", "ABB700C99999"]);
    }

    #[test]
    fn parse_configuration_device_name() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        let (_uuid, sysap) = cfg.first_sysap().expect("one sysap");
        let dev = sysap.devices.get("ABB700C12345").expect("device");
        assert_eq!(dev.display_name.as_deref(), Some("Living Room Light"));
    }

    #[test]
    fn channel_function_resolves() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        let (_u, s) = cfg.first_sysap().expect("sysap");
        let ch = s
            .devices
            .get("ABB700C12345")
            .and_then(|d| d.channels.get("ch0000"))
            .expect("channel");
        assert_eq!(ch.function(), Some(Function::DimmingActuator));
    }

    #[test]
    fn datapoint_value_extracted() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        let (_u, s) = cfg.first_sysap().expect("sysap");
        let dp = s
            .devices
            .get("ABB700C12345")
            .and_then(|d| d.channels.get("ch0000"))
            .and_then(|c| c.datapoints.get("odp0001"))
            .expect("datapoint");
        assert_eq!(dp.value.as_deref(), Some("80"));
    }

    #[test]
    fn function_id_parses_hex_with_and_without_prefix() {
        assert_eq!(parse_function_id("0012"), Some(0x0012));
        assert_eq!(parse_function_id("0x0007"), Some(0x0007));
        assert_eq!(parse_function_id("zz"), None);
    }

    #[test]
    fn missing_display_name_tolerated() {
        let json = r#"{ "u": { "devices": { "S": { "channels": {} } } } }"#;
        let cfg = ConfigurationResponse::parse(json).expect("parse");
        let (_u, s) = cfg.first_sysap().expect("sysap");
        assert!(s.devices.get("S").expect("dev").display_name.is_none());
    }

    #[test]
    fn sysap_count_is_one() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        assert_eq!(cfg.sysap_count(), 1);
    }
}
