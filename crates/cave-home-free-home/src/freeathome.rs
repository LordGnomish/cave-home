// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/freeathome.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! `FreeAtHome` — top-level facade that owns the device cache and
//! routes WebSocket deltas onto the relevant channel.

use std::collections::HashMap;

use parking_lot::RwLock;

use crate::channels::Channel;
use crate::device::Device;
use crate::ws::WsUpdate;

/// Cave-home top-level free@home runtime facade. Internally a single-writer
/// `RwLock` over the device cache; the public API mirrors upstream's
/// async-Python class (which is single-task already).
#[derive(Debug, Default)]
pub struct FreeAtHome {
    devices: RwLock<HashMap<String, Device>>,
}

impl FreeAtHome {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the device cache (typically called once after loading the
    /// SysAP configuration document).
    pub fn install_devices<I: IntoIterator<Item = Device>>(&self, devices: I) {
        let mut guard = self.devices.write();
        guard.clear();
        for d in devices {
            guard.insert(d.device_serial.clone(), d);
        }
    }

    /// Total devices known.
    #[must_use]
    pub fn device_count(&self) -> usize {
        self.devices.read().len()
    }

    /// Returns a clone of the channel (if any) for `serial/channel_id`.
    #[must_use]
    pub fn channel(&self, device_serial: &str, channel_id: &str) -> Option<Channel> {
        self.devices
            .read()
            .get(device_serial)
            .and_then(|d| d.channels.get(channel_id).cloned())
    }

    /// Apply a parsed websocket update — mirrors upstream's `update()`.
    ///
    /// Returns the number of `(device, channel, datapoint)` deltas that
    /// matched something in the cache.
    pub fn apply_update(&self, update: &WsUpdate) -> usize {
        let mut matched = 0;
        let mut guard = self.devices.write();
        for (serial, channel_id, datapoint, value) in update.iter_datapoints() {
            if let Some(dev) = guard.get_mut(serial) {
                if let Some(channel) = dev.channels.get_mut(channel_id) {
                    if channel.apply_delta(datapoint, value) {
                        matched += 1;
                    }
                }
            }
        }
        matched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::{ChannelBase, Datapoint, SwitchActuator};
    use crate::pairing::Pairing;

    fn one_switch_device() -> Device {
        let mut outputs = HashMap::new();
        outputs.insert(
            "odp0000".into(),
            Datapoint {
                pairing_id: Pairing::AlInfoOnOff.value(),
                value: Some("0".into()),
            },
        );
        let base = ChannelBase {
            device_serial: "ABB7F500BCFB".into(),
            channel_id: "ch0000".into(),
            channel_name: "Mutfak Tavan".into(),
            inputs: HashMap::new(),
            outputs,
            floor_name: Some("Zemin".into()),
            room_name: Some("Mutfak".into()),
        };
        let mut sw = SwitchActuator {
            base,
            state: None,
        };
        sw.refresh_state();
        let mut d = Device::new(
            "ABB7F500BCFB".into(),
            "B002".into(),
            "Mutfak Aktör".into(),
        );
        d.channels.insert("ch0000".into(), Channel::Switch(sw));
        d
    }

    #[test]
    fn install_devices_populates_cache() {
        let fah = FreeAtHome::new();
        fah.install_devices([one_switch_device()]);
        assert_eq!(fah.device_count(), 1);
        let ch = fah.channel("ABB7F500BCFB", "ch0000").expect("channel");
        match ch {
            Channel::Switch(sw) => assert_eq!(sw.state, Some(false)),
            _ => panic!(),
        }
    }

    #[test]
    fn apply_update_routes_delta_to_channel() {
        let fah = FreeAtHome::new();
        fah.install_devices([one_switch_device()]);
        let raw = r#"{
            "00000000-0000-0000-0000-000000000000": {
                "datapoints": { "ABB7F500BCFB/ch0000/odp0000": "1" },
                "devicesAdded": [],
                "devicesRemoved": [],
                "scenesTriggered": {}
            }
        }"#;
        let u = WsUpdate::from_json(raw).unwrap();
        let n = fah.apply_update(&u);
        assert_eq!(n, 1);
        match fah.channel("ABB7F500BCFB", "ch0000").unwrap() {
            Channel::Switch(sw) => assert_eq!(sw.state, Some(true)),
            _ => panic!(),
        }
    }

    #[test]
    fn apply_update_ignores_unknown_devices() {
        let fah = FreeAtHome::new();
        fah.install_devices([one_switch_device()]);
        let raw = r#"{
            "sysap": {
                "datapoints": { "OTHER/ch0000/odp0000": "1" },
                "devicesAdded": [], "devicesRemoved": [], "scenesTriggered": {}
            }
        }"#;
        assert_eq!(fah.apply_update(&WsUpdate::from_json(raw).unwrap()), 0);
    }
}
