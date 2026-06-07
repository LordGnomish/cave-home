// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! WebSocket push-event parsing.
//!
//! The SysAP `fhapi` WebSocket pushes a JSON object keyed by SysAP UUID. Each
//! value carries the datapoints that changed (keyed `serial/channel/datapoint`
//! → wire value) plus added/removed device serials. We flatten that into a list
//! of typed [`FreeAtHomeEvent`]s and silently drop addresses that don't parse,
//! so one bad key never sinks an otherwise-good frame.

use std::collections::BTreeMap;

use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial};
use serde::Deserialize;

use crate::error::Result;

/// A single datapoint value change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatapointUpdate {
    serial: DeviceSerial,
    channel: ChannelId,
    datapoint: DatapointId,
    value: String,
}

impl DatapointUpdate {
    /// Construct an update.
    pub const fn new(
        serial: DeviceSerial,
        channel: ChannelId,
        datapoint: DatapointId,
        value: String,
    ) -> Self {
        Self {
            serial,
            channel,
            datapoint,
            value,
        }
    }

    /// The owning device serial.
    pub const fn serial(&self) -> &DeviceSerial {
        &self.serial
    }

    /// The channel that changed.
    pub const fn channel(&self) -> ChannelId {
        self.channel
    }

    /// The datapoint that changed.
    pub const fn datapoint(&self) -> DatapointId {
        self.datapoint
    }

    /// The new wire value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The canonical `serial/channel/datapoint` address.
    pub fn address(&self) -> String {
        format!("{}/{}/{}", self.serial, self.channel, self.datapoint)
    }
}

/// A typed event decoded from a WebSocket frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeAtHomeEvent {
    /// A datapoint reported a new value.
    DatapointUpdate(DatapointUpdate),
    /// A device joined the SysAP.
    DeviceAdded(DeviceSerial),
    /// A device left the SysAP.
    DeviceRemoved(DeviceSerial),
}

impl FreeAtHomeEvent {
    /// Borrow the inner [`DatapointUpdate`] if this is one.
    pub const fn as_datapoint_update(&self) -> Option<&DatapointUpdate> {
        match self {
            Self::DatapointUpdate(u) => Some(u),
            _ => None,
        }
    }
}

/// The per-SysAP body of a WebSocket frame.
#[derive(Debug, Default, Deserialize)]
struct WsBody {
    #[serde(default)]
    datapoints: BTreeMap<String, String>,
    #[serde(rename = "devicesAdded", default)]
    devices_added: Vec<String>,
    #[serde(rename = "devicesRemoved", default)]
    devices_removed: Vec<String>,
}

/// Split a `serial/channel/datapoint` address into typed ids.
///
/// Returns `None` if the shape or any component is invalid.
pub fn parse_datapoint_address(addr: &str) -> Option<(DeviceSerial, ChannelId, DatapointId)> {
    let mut parts = addr.split('/');
    let serial = DeviceSerial::parse(parts.next()?).ok()?;
    let channel = ChannelId::parse(parts.next()?).ok()?;
    let datapoint = DatapointId::parse(parts.next()?).ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((serial, channel, datapoint))
}

/// Parse a raw WebSocket text frame into a flat list of events.
pub fn parse_ws_frame(json: &str) -> Result<Vec<FreeAtHomeEvent>> {
    let frame: BTreeMap<String, WsBody> = serde_json::from_str(json)?;
    let mut events = Vec::new();
    for body in frame.values() {
        for (addr, value) in &body.datapoints {
            if let Some((serial, channel, datapoint)) = parse_datapoint_address(addr) {
                events.push(FreeAtHomeEvent::DatapointUpdate(DatapointUpdate::new(
                    serial,
                    channel,
                    datapoint,
                    value.clone(),
                )));
            }
        }
        for serial in &body.devices_added {
            if let Ok(s) = DeviceSerial::parse(serial) {
                events.push(FreeAtHomeEvent::DeviceAdded(s));
            }
        }
        for serial in &body.devices_removed {
            if let Ok(s) = DeviceSerial::parse(serial) {
                events.push(FreeAtHomeEvent::DeviceRemoved(s));
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{ChannelId, DatapointId, Direction};

    const WS_FRAME: &str = r#"{
      "00000000-0000-0000-0000-000000000000": {
        "datapoints": {
          "ABB700C12345/ch0000/odp0000": "1",
          "ABB700C12345/ch0000/odp0001": "75"
        },
        "devicesAdded": [],
        "devicesRemoved": []
      }
    }"#;

    #[test]
    fn parses_datapoint_updates() {
        let evs = parse_ws_frame(WS_FRAME).expect("parse");
        let updates: Vec<_> = evs
            .iter()
            .filter_map(FreeAtHomeEvent::as_datapoint_update)
            .collect();
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn datapoint_update_carries_value() {
        let evs = parse_ws_frame(WS_FRAME).expect("parse");
        let u = evs
            .iter()
            .filter_map(FreeAtHomeEvent::as_datapoint_update)
            .find(|u| u.datapoint() == DatapointId::new(Direction::Output, 1))
            .expect("odp0001");
        assert_eq!(u.value(), "75");
        assert_eq!(u.channel(), ChannelId::new(0));
        assert_eq!(u.serial().as_str(), "ABB700C12345");
    }

    #[test]
    fn parse_address_triple() {
        let (s, c, d) =
            parse_datapoint_address("ABB700C12345/ch0003/idp0001").expect("address");
        assert_eq!(s.as_str(), "ABB700C12345");
        assert_eq!(c, ChannelId::new(3));
        assert_eq!(d, DatapointId::new(Direction::Input, 1));
    }

    #[test]
    fn invalid_address_is_skipped_not_fatal() {
        let json = r#"{ "u": { "datapoints": {
            "garbage": "1",
            "ABB700C12345/ch0000/odp0000": "1"
        } } }"#;
        let evs = parse_ws_frame(json).expect("parse");
        assert_eq!(
            evs.iter()
                .filter_map(FreeAtHomeEvent::as_datapoint_update)
                .count(),
            1
        );
    }

    #[test]
    fn empty_frame_yields_no_events() {
        let evs = parse_ws_frame(r#"{ "u": {} }"#).expect("parse");
        assert!(evs.is_empty());
    }

    #[test]
    fn devices_added_and_removed() {
        let json = r#"{ "u": {
            "devicesAdded": ["ABB700C12345"],
            "devicesRemoved": ["ABB700C99999"]
        } }"#;
        let evs = parse_ws_frame(json).expect("parse");
        assert!(evs.iter().any(
            |e| matches!(e, FreeAtHomeEvent::DeviceAdded(s) if s.as_str() == "ABB700C12345")
        ));
        assert!(evs.iter().any(
            |e| matches!(e, FreeAtHomeEvent::DeviceRemoved(s) if s.as_str() == "ABB700C99999")
        ));
    }

    #[test]
    fn malformed_json_errors() {
        assert!(parse_ws_frame("not json").is_err());
    }
}
