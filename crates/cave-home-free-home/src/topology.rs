// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The typed free@home topology and the "get-all" parser.
//!
//! The System Access Point exposes its whole installation as one big devices
//! tree (the "get-all" configuration response). cave-home parses that into a
//! strongly-typed [`SysAp`] → [`Device`] → [`Channel`] → [`Datapoint`] model
//! once, so downstream code walks typed structs rather than raw JSON.
//!
//! The documented response shape is, abbreviated:
//!
//! ```json
//! { "<sysApId>": { "devices": {
//!     "ABB700C12345": { "displayName": "Wohnzimmer Decke", "channels": {
//!         "ch0003": { "functionID": "0012", "displayName": "Licht",
//!                     "room": "Wohnzimmer", "floor": "EG",
//!                     "outputs": { "odp0000": { "pairingID": 256, "value": "1" } },
//!                     "inputs":  { "idp0000": { "pairingID": 1,   "value": "0" } } }
//!     }}
//! }}}
//! ```
//!
//! To stay std-only with no external crate (Charter constraint, Phase 1), this
//! module carries a small tolerant JSON reader sufficient for that shape:
//! objects, strings and the scalars free@home uses. Numbers and unknown keys
//! are read as raw strings; nothing here needs floats from JSON.

use std::collections::BTreeMap;

use crate::function::Function;
use crate::id::{ChannelId, DatapointId, DeviceSerial, Direction};
use crate::pairing::Pairing;

mod json;
use json::Json;

/// Why a get-all response failed to parse into a topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The JSON itself was malformed.
    Malformed(String),
    /// A required object/field was missing or the wrong type.
    Shape(String),
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "malformed response: {m}"),
            Self::Shape(m) => write!(f, "unexpected response shape: {m}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// One datapoint in the typed tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datapoint {
    pub id: DatapointId,
    pub pairing: Option<Pairing>,
    pub raw_pairing_id: u16,
    pub value: Option<String>,
}

/// A channel: a single function on a device, with its datapoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    id: ChannelId,
    function: Function,
    room: Option<String>,
    floor: Option<String>,
    datapoints: Vec<Datapoint>,
}

impl Channel {
    /// Construct a channel with no datapoints (used in mapping tests + builders).
    #[must_use]
    pub const fn new(
        id: ChannelId,
        function: Function,
        room: Option<String>,
        floor: Option<String>,
    ) -> Self {
        Self {
            id,
            function,
            room,
            floor,
            datapoints: Vec::new(),
        }
    }

    #[must_use]
    pub const fn id(&self) -> ChannelId {
        self.id
    }

    #[must_use]
    pub const fn function(&self) -> Function {
        self.function
    }

    #[must_use]
    pub fn room(&self) -> Option<&str> {
        self.room.as_deref()
    }

    #[must_use]
    pub fn floor(&self) -> Option<&str> {
        self.floor.as_deref()
    }

    #[must_use]
    pub fn datapoints(&self) -> &[Datapoint] {
        &self.datapoints
    }

    /// The first datapoint carrying a given pairing role, if any.
    #[must_use]
    pub fn datapoint_for(&self, pairing: Pairing) -> Option<&Datapoint> {
        self.datapoints.iter().find(|d| d.pairing == Some(pairing))
    }
}

/// A device: a serial + its channels + a display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: DeviceSerial,
    pub display_name: Option<String>,
    pub channels: Vec<Channel>,
}

/// The whole installation as parsed from a get-all response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysAp {
    pub id: String,
    pub devices: Vec<Device>,
}

impl SysAp {
    /// Parse a get-all configuration response.
    ///
    /// Channels whose `functionID` is not in the curated [`Function`] set are
    /// skipped (they are out of Phase-1 scope, not an error). Datapoints with
    /// an unrecognised pairing ID are kept with `pairing: None` so the raw id
    /// round-trips.
    ///
    /// # Errors
    /// Returns [`ParseError`] if the JSON is malformed or the top-level shape
    /// (a single sysAP object containing `devices`) is missing.
    pub fn parse_get_all(input: &str) -> Result<Self, ParseError> {
        let root = Json::parse(input).map_err(ParseError::Malformed)?;
        let top = root
            .as_object()
            .ok_or_else(|| ParseError::Shape("top level is not an object".into()))?;
        let (sysap_id, sysap) = top
            .iter()
            .next()
            .ok_or_else(|| ParseError::Shape("no sysAP entry".into()))?;
        let sysap = sysap
            .as_object()
            .ok_or_else(|| ParseError::Shape("sysAP entry is not an object".into()))?;
        let device_map = sysap
            .get("devices")
            .and_then(Json::as_object)
            .ok_or_else(|| ParseError::Shape("missing devices object".into()))?;

        let mut devices = Vec::new();
        for (raw_serial, device_json) in device_map {
            // A serial we cannot parse is skipped, not fatal.
            let Ok(serial) = DeviceSerial::parse(raw_serial) else {
                continue;
            };
            let Some(fields) = device_json.as_object() else {
                continue;
            };
            let display_name = fields
                .get("displayName")
                .and_then(Json::as_str)
                .map(str::to_string);
            let channels = parse_channels(fields);
            devices.push(Device {
                serial,
                display_name,
                channels,
            });
        }

        Ok(Self {
            id: sysap_id.clone(),
            devices,
        })
    }

    /// Total channel count across all devices.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.devices.iter().map(|d| d.channels.len()).sum()
    }
}

fn parse_channels(device_obj: &BTreeMap<String, Json>) -> Vec<Channel> {
    let Some(channels_obj) = device_obj.get("channels").and_then(Json::as_object) else {
        return Vec::new();
    };
    let mut channels = Vec::new();
    for (channel_id_str, channel_json) in channels_obj {
        let Ok(channel_id) = ChannelId::parse(channel_id_str) else {
            continue;
        };
        let Some(fields) = channel_json.as_object() else {
            continue;
        };
        let Some(function) = fields
            .get("functionID")
            .and_then(Json::as_str)
            .and_then(|s| u16::from_str_radix(s.trim(), 16).ok())
            .and_then(Function::from_id)
        else {
            // Out-of-scope or unparseable function: skip this channel.
            continue;
        };
        let room = fields
            .get("room")
            .and_then(Json::as_str)
            .map(str::to_string);
        let floor = fields
            .get("floor")
            .and_then(Json::as_str)
            .map(str::to_string);

        let mut datapoints = Vec::new();
        parse_datapoints(fields.get("inputs"), Direction::Input, &mut datapoints);
        parse_datapoints(
            fields.get("outputs"),
            Direction::Output,
            &mut datapoints,
        );

        channels.push(Channel {
            id: channel_id,
            function,
            room,
            floor,
            datapoints,
        });
    }
    channels
}

fn parse_datapoints(group: Option<&Json>, expected: Direction, out: &mut Vec<Datapoint>) {
    let Some(obj) = group.and_then(Json::as_object) else {
        return;
    };
    for (dp_id_str, dp_json) in obj {
        let Ok(id) = DatapointId::parse(dp_id_str) else {
            continue;
        };
        // Trust the group it appeared under, but only keep ids whose own
        // direction prefix matches — guards against a mis-filed entry.
        if id.direction() != expected {
            continue;
        }
        let Some(dp_obj) = dp_json.as_object() else {
            continue;
        };
        let raw_pairing_id = dp_obj
            .get("pairingID")
            .and_then(Json::as_str)
            .and_then(|s| s.trim().parse::<u16>().ok())
            .unwrap_or(0);
        let value = dp_obj
            .get("value")
            .and_then(Json::as_str)
            .map(str::to_string);
        out.push(Datapoint {
            id,
            pairing: Pairing::from_id(raw_pairing_id),
            raw_pairing_id,
            value,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
    {
      "00000000-0000-0000-0000-000000000000": {
        "devices": {
          "ABB700C12345": {
            "displayName": "Wohnzimmer Decke",
            "channels": {
              "ch0003": {
                "functionID": "0012",
                "room": "Wohnzimmer",
                "floor": "EG",
                "inputs": { "idp0000": { "pairingID": 1, "value": "0" },
                            "idp0002": { "pairingID": 17, "value": "50" } },
                "outputs": { "odp0000": { "pairingID": 256, "value": "1" } }
              },
              "ch0001": {
                "functionID": "0061",
                "room": "Wohnzimmer",
                "inputs": { "idp0000": { "pairingID": 32, "value": "0" } },
                "outputs": { "odp0001": { "pairingID": 133, "value": "75" } }
              },
              "ch00ff": {
                "functionID": "9999",
                "outputs": {}
              }
            }
          }
        }
      }
    }
    "#;

    #[test]
    fn parses_top_level_and_device() {
        let sysap = SysAp::parse_get_all(SAMPLE).expect("parses");
        assert_eq!(sysap.id, "00000000-0000-0000-0000-000000000000");
        assert_eq!(sysap.devices.len(), 1);
        let dev = &sysap.devices[0];
        assert_eq!(dev.serial.as_str(), "ABB700C12345");
        assert_eq!(dev.display_name.as_deref(), Some("Wohnzimmer Decke"));
    }

    #[test]
    fn skips_unknown_function_channel() {
        let sysap = SysAp::parse_get_all(SAMPLE).expect("parses");
        // ch00ff has functionID 9999 (not curated) and is dropped.
        assert_eq!(sysap.channel_count(), 2);
    }

    #[test]
    fn channel_function_and_room_decoded() {
        let sysap = SysAp::parse_get_all(SAMPLE).expect("parses");
        let dev = &sysap.devices[0];
        let dimmer = dev
            .channels
            .iter()
            .find(|c| c.id() == ChannelId::new(3))
            .expect("ch0003 present");
        assert_eq!(dimmer.function(), Function::DimmingActuator);
        assert_eq!(dimmer.room(), Some("Wohnzimmer"));
        assert_eq!(dimmer.floor(), Some("EG"));
    }

    #[test]
    fn datapoints_split_input_output_and_map_pairing() {
        let sysap = SysAp::parse_get_all(SAMPLE).expect("parses");
        let dev = &sysap.devices[0];
        let dimmer = dev
            .channels
            .iter()
            .find(|c| c.id() == ChannelId::new(3))
            .expect("ch0003");
        // Switch-on input present, mapped to a pairing.
        let on = dimmer
            .datapoint_for(Pairing::SwitchOnOff)
            .expect("switch on/off input");
        assert!(on.id.is_input());
        assert_eq!(on.value.as_deref(), Some("0"));
        // Brightness setpoint (pairingID 17 = 0x11).
        let bri = dimmer
            .datapoint_for(Pairing::SetBrightness)
            .expect("brightness input");
        assert_eq!(bri.value.as_deref(), Some("50"));
        // Reported on/off output (pairingID 256 = 0x100).
        let info = dimmer
            .datapoint_for(Pairing::InfoOnOff)
            .expect("info output");
        assert_eq!(info.id.direction(), Direction::Output);
        assert_eq!(info.value.as_deref(), Some("1"));
    }

    #[test]
    fn blind_channel_position_output() {
        let sysap = SysAp::parse_get_all(SAMPLE).expect("parses");
        let blind = sysap.devices[0]
            .channels
            .iter()
            .find(|c| c.function() == Function::BlindActuator)
            .expect("blind channel");
        let pos = blind
            .datapoint_for(Pairing::InfoBlindPosition)
            .expect("position output");
        assert_eq!(pos.value.as_deref(), Some("75"));
    }

    #[test]
    fn unknown_pairing_id_kept_as_raw() {
        let json = r#"
        {"sid":{"devices":{"ABB700C12345":{"channels":{"ch0000":{
            "functionID":"0007",
            "outputs":{"odp0009":{"pairingID":40000,"value":"x"}}
        }}}}}}"#;
        let sysap = SysAp::parse_get_all(json).expect("parses");
        let dp = &sysap.devices[0].channels[0].datapoints()[0];
        assert_eq!(dp.pairing, None);
        assert_eq!(dp.raw_pairing_id, 40000);
        assert_eq!(dp.id.to_string(), "odp0009");
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(
            SysAp::parse_get_all("{not json"),
            Err(ParseError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_missing_devices() {
        assert!(matches!(
            SysAp::parse_get_all(r#"{"sid":{}}"#),
            Err(ParseError::Shape(_))
        ));
    }
}
