// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/device.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! free@home `Device` model.

use std::collections::HashMap;

use crate::channels::Channel;

/// SysAP interface flavour (`Interface` enum upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Interface {
    /// Wired free@home over twisted-pair bus.
    Wired,
    /// free@home wireless (RF) device.
    Wireless,
    /// Virtual device defined inside the SysAP.
    Virtual,
    #[default]
    Undefined,
}

/// A free@home device (one physical actuator/sensor on the bus). One
/// device hosts one or more [`Channel`]s. `PartialEq` only because
/// `Channel` contains `f64` temperature readings; `Eq` would be wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub device_serial: String,
    pub device_id: String,
    pub display_name: String,
    pub interface: Interface,
    pub unresponsive: bool,
    pub unresponsive_counter: u32,
    pub defect: bool,
    pub floor_name: Option<String>,
    pub room_name: Option<String>,
    pub native_id: Option<String>,
    pub channels: HashMap<String, Channel>,
}

impl Device {
    /// Mirror of upstream `Device(...)` constructor.
    #[must_use]
    pub fn new(device_serial: String, device_id: String, display_name: String) -> Self {
        Self {
            device_serial,
            device_id,
            display_name,
            interface: Interface::default(),
            unresponsive: false,
            unresponsive_counter: 0,
            defect: false,
            floor_name: None,
            room_name: None,
            native_id: None,
            channels: HashMap::new(),
        }
    }

    /// Whether this device represents a virtual SysAP entity.
    #[must_use]
    pub fn is_virtual(&self) -> bool {
        self.interface == Interface::Virtual
    }

    /// Clear all channels — mirror of upstream `clear_channels()`.
    pub fn clear_channels(&mut self) {
        self.channels.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_device_has_no_channels() {
        let d = Device::new("ABB7F500BCFB".into(), "B002".into(), "Mutfak".into());
        assert!(d.channels.is_empty());
        assert_eq!(d.interface, Interface::Undefined);
        assert!(!d.is_virtual());
    }

    #[test]
    fn virtual_device_flag() {
        let mut d = Device::new("ABBVIRT0001".into(), "FFFF".into(), "Sahne".into());
        d.interface = Interface::Virtual;
        assert!(d.is_virtual());
    }
}
