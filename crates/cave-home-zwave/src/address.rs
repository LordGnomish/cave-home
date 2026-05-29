// SPDX-License-Identifier: Apache-2.0
//! Node / endpoint addressing and a coarse device-role hint.
//!
//! A Z-Wave network addresses a device by its 8-bit **node id** (1..=232 in a
//! classic network; node 0 is reserved/broadcast-context and never a real
//! device). The Multi Channel Command Class adds a second axis — an **endpoint**
//! — so one physical node can expose several independent sub-devices (e.g. the
//! two relays of a double switch). Endpoint 0 means "the node itself".
//!
//! This module is pure addressing: no transport, no controller. The
//! [`DeviceRole`] hint is the *household-level* idea of what a device is for —
//! it is what the grandma-friendly label layer keys off, never a protocol term.

use crate::error::{ZwaveError, ZwaveResult};

/// A node + endpoint address.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Address {
    node_id: u8,
    endpoint: u8,
}

impl Address {
    /// Build an address for the node itself (endpoint 0).
    ///
    /// # Errors
    /// Returns [`ZwaveError::OutOfRange`] if `node_id` is 0 (reserved).
    pub fn node(node_id: u8) -> ZwaveResult<Self> {
        Self::at_endpoint(node_id, 0)
    }

    /// Build an address for a specific endpoint of a node.
    ///
    /// # Errors
    /// Returns [`ZwaveError::OutOfRange`] if `node_id` is 0, or if `endpoint`
    /// exceeds the Multi Channel maximum of 127 (the high bit of the endpoint
    /// field is reserved by the specification).
    pub fn at_endpoint(node_id: u8, endpoint: u8) -> ZwaveResult<Self> {
        if node_id == 0 {
            return Err(ZwaveError::OutOfRange {
                field: "node_id",
                value: 0,
            });
        }
        if endpoint > 127 {
            return Err(ZwaveError::OutOfRange {
                field: "endpoint",
                value: u32::from(endpoint),
            });
        }
        Ok(Self { node_id, endpoint })
    }

    /// The node id.
    #[must_use]
    pub const fn node_id(self) -> u8 {
        self.node_id
    }

    /// The endpoint (0 = the node itself).
    #[must_use]
    pub const fn endpoint(self) -> u8 {
        self.endpoint
    }

    /// Whether this address targets the root device rather than a sub-endpoint.
    #[must_use]
    pub const fn is_root(self) -> bool {
        self.endpoint == 0
    }
}

/// A household-level idea of what a device does. This is a *hint* derived from
/// the Command Classes a device reports; it drives the grandma-friendly label
/// and nothing in the protocol depends on it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DeviceRole {
    /// An on/off switch (a lamp, a socket).
    Switch,
    /// A dimmer or anything with a 0–100% level.
    Dimmer,
    /// A colour light.
    Light,
    /// A sensor that reports a measured number (temperature, humidity, …).
    Sensor,
    /// A door/window/motion-style yes/no sensor.
    Contact,
    /// A heating/cooling setpoint device (a thermostat).
    Thermostat,
    /// A battery-powered device whose charge we track.
    BatteryDevice,
    /// A device whose purpose we cannot infer yet.
    Unknown,
}

impl DeviceRole {
    /// A stable, lowercase, household-friendly word for this role. Used to build
    /// localized labels; never a protocol identifier.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::Dimmer => "dimmer",
            Self::Light => "light",
            Self::Sensor => "sensor",
            Self::Contact => "contact",
            Self::Thermostat => "thermostat",
            Self::BatteryDevice => "battery",
            Self::Unknown => "device",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_zero_is_rejected() {
        assert_eq!(
            Address::node(0),
            Err(ZwaveError::OutOfRange {
                field: "node_id",
                value: 0
            })
        );
    }

    #[test]
    fn root_and_endpoint_addresses() {
        let root = Address::node(5).expect("node 5 is valid");
        assert!(root.is_root());
        assert_eq!(root.node_id(), 5);
        assert_eq!(root.endpoint(), 0);

        let ep = Address::at_endpoint(5, 2).expect("endpoint 2 is valid");
        assert!(!ep.is_root());
        assert_eq!(ep.endpoint(), 2);
    }

    #[test]
    fn reserved_endpoint_high_bit_rejected() {
        assert!(Address::at_endpoint(5, 128).is_err());
        assert!(Address::at_endpoint(5, 127).is_ok());
    }

    #[test]
    fn role_slugs_are_plain_words() {
        assert_eq!(DeviceRole::Dimmer.slug(), "dimmer");
        assert_eq!(DeviceRole::Unknown.slug(), "device");
    }
}
