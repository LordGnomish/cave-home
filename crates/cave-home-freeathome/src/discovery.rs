// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Device discovery: project a parsed SysAP topology into cave-home devices.
//!
//! The SysAP `GET configuration` response is parsed by the brain
//! ([`cave_home_free_home::SysAp::parse_get_all`]) into a typed device → channel
//! → datapoint tree. Discovery flattens that into one [`crate::device::Device`]
//! per in-scope channel — every actuator (Aktor: switch, dimmer, blind, room
//! controller) and every sensor (Sensor) the household can see — carrying its
//! function, room and last-reported state. The grandma-friendly device kind and
//! value semantics come from the brain; discovery only assembles the list.

use cave_home_free_home::SysAp;

use crate::device::Device;

/// Flatten a parsed SysAP topology into the devices it exposes.
///
/// One [`Device`] is produced per channel whose function is in cave-home's
/// curated set; channels with unknown functions are already dropped by the
/// topology parser. Devices are returned in deterministic order (by serial,
/// then by channel index). The friendly name is the device's display name,
/// falling back to its serial.
pub fn discover(sysap: &SysAp) -> Vec<Device> {
    let mut devices = Vec::new();
    for device in &sysap.devices {
        let name = device
            .display_name
            .clone()
            .unwrap_or_else(|| device.serial.as_str().to_string());
        for channel in &device.channels {
            devices.push(Device::new(
                device.serial.clone(),
                channel.clone(),
                name.clone(),
            ));
        }
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{DeviceKind, SysAp};

    // A realistic single-SysAP configuration: a dimmable light (on, 80 %), a
    // blind reporting 50 % position, a room thermostat reporting 21.5 °C, and a
    // read-only wall switch sensor. Datapoint groups use the real inputs/outputs
    // shape the SysAP returns; pairingID values are the protocol constants.
    const CONFIG: &str = r#"
    {
      "00000000-0000-0000-0000-000000000000": {
        "devices": {
          "ABB700C12345": {
            "displayName": "Wohnzimmer Decke",
            "channels": {
              "ch0000": {
                "functionID": "0012",
                "room": "Wohnzimmer",
                "inputs":  { "idp0000": { "pairingID": 1,   "value": "1" } },
                "outputs": { "odp0000": { "pairingID": 256, "value": "1" },
                             "odp0001": { "pairingID": 272, "value": "80" } }
              }
            }
          },
          "ABB700C22222": {
            "displayName": "Schlafzimmer Rollladen",
            "channels": {
              "ch0001": {
                "functionID": "0061",
                "room": "Schlafzimmer",
                "outputs": { "odp0001": { "pairingID": 133, "value": "50" } }
              }
            }
          },
          "ABB700C33333": {
            "displayName": "Bad Thermostat",
            "channels": {
              "ch0002": {
                "functionID": "0023",
                "outputs": { "odp0000": { "pairingID": 304, "value": "21.5" } }
              }
            }
          },
          "ABB700C44444": {
            "displayName": "Flur Taster",
            "channels": {
              "ch0000": {
                "functionID": "0000",
                "outputs": { "odp0000": { "pairingID": 256, "value": "0" } }
              }
            }
          }
        }
      }
    }"#;

    fn discovered() -> Vec<Device> {
        let sysap = SysAp::parse_get_all(CONFIG).expect("config parses");
        discover(&sysap)
    }

    #[test]
    fn discovers_one_device_per_channel() {
        assert_eq!(discovered().len(), 4);
    }

    #[test]
    fn discovers_every_device_kind() {
        use crate::device::FreeAtHomeDevice as _;
        let kinds: Vec<DeviceKind> = discovered().iter().map(Device::kind).collect();
        assert!(kinds.contains(&DeviceKind::Light));
        assert!(kinds.contains(&DeviceKind::Cover));
        assert!(kinds.contains(&DeviceKind::Climate));
        assert!(kinds.contains(&DeviceKind::Sensor));
    }

    #[test]
    fn light_carries_name_room_and_on_state() {
        use crate::device::FreeAtHomeDevice as _;
        let light = discovered()
            .into_iter()
            .find(|d| d.kind() == DeviceKind::Light)
            .expect("a light");
        assert_eq!(light.friendly_name(), "Wohnzimmer Decke");
        assert_eq!(light.room(), Some("Wohnzimmer"));
        assert_eq!(light.display_state(), "on");
    }

    #[test]
    fn cover_reports_position_value() {
        use crate::device::FreeAtHomeDevice as _;
        let cover = discovered()
            .into_iter()
            .find(|d| d.kind() == DeviceKind::Cover)
            .expect("a cover");
        assert_eq!(cover.primary_value(), Some("50"));
        assert_eq!(cover.display_state(), "50");
    }

    #[test]
    fn climate_reports_temperature_value() {
        use crate::device::FreeAtHomeDevice as _;
        let climate = discovered()
            .into_iter()
            .find(|d| d.kind() == DeviceKind::Climate)
            .expect("a thermostat");
        assert_eq!(climate.display_state(), "21.5");
    }

    #[test]
    fn sensor_is_off_and_read_only() {
        use crate::device::FreeAtHomeDevice as _;
        let sensor = discovered()
            .into_iter()
            .find(|d| d.kind() == DeviceKind::Sensor)
            .expect("a sensor");
        assert_eq!(sensor.display_state(), "off");
        assert!(!sensor.is_controllable());
    }

    #[test]
    fn order_is_deterministic_by_serial() {
        use crate::device::FreeAtHomeDevice as _;
        let serials: Vec<String> = discovered()
            .iter()
            .map(|d| d.serial().as_str().to_string())
            .collect();
        let mut sorted = serials.clone();
        sorted.sort();
        assert_eq!(serials, sorted);
    }
}
