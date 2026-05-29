// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! free@home pairing IDs — the *role* of a datapoint.
//!
//! A channel carries several datapoints; each is tagged with a "pairing ID"
//! (the `pairingID` field in the System Access Point's device tree) that says
//! what the datapoint *means*: the switch on/off input, the brightness-state
//! output, the temperature setpoint input, the move-up/down input, etc. These
//! IDs are Busch-Jaeger protocol constants, pinned here.
//!
//! Each role also declares the [`ValueShape`] it carries — boolean, percentage
//! or temperature — which the [`crate::command`] layer uses to validate a
//! control value before it is encoded for the wire.

use crate::value::{Value, ValueError};

/// The shape of value a pairing role carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    /// Boolean (`"0"`/`"1"`).
    Bool,
    /// Percentage `0..=100`.
    Percent,
    /// Temperature in degrees Celsius.
    Temperature,
}

/// A curated set of documented free@home pairing IDs (Phase-1 roles).
///
/// The numeric values are protocol-stable and match what the System Access
/// Point returns in its device tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Pairing {
    /// Switch on/off — **input** (a command target). Boolean.
    SwitchOnOff = 0x0001,
    /// Reported on/off state — **output**. Boolean.
    InfoOnOff = 0x0100,
    /// Absolute brightness setpoint — **input**. Percent.
    SetBrightness = 0x0011,
    /// Reported actual brightness — **output**. Percent.
    InfoBrightness = 0x0110,
    /// Move up / down — **input**. Boolean (`0` up/open, `1` down/close).
    MoveUpDown = 0x0020,
    /// Absolute blind position setpoint — **input**. Percent.
    SetBlindPosition = 0x0023,
    /// Reported blind position — **output**. Percent.
    InfoBlindPosition = 0x0085,
    /// Target room temperature — **input**. Temperature.
    SetTargetTemperature = 0x0140,
    /// Measured room temperature — **output**. Temperature.
    InfoCurrentTemperature = 0x0130,
}

impl Pairing {
    /// The protocol pairing ID as exposed in the device tree.
    #[must_use]
    pub const fn id(self) -> u16 {
        self as u16
    }

    /// Map a numeric pairing ID to a [`Pairing`].
    #[must_use]
    pub const fn from_id(id: u16) -> Option<Self> {
        match id {
            0x0001 => Some(Self::SwitchOnOff),
            0x0100 => Some(Self::InfoOnOff),
            0x0011 => Some(Self::SetBrightness),
            0x0110 => Some(Self::InfoBrightness),
            0x0020 => Some(Self::MoveUpDown),
            0x0023 => Some(Self::SetBlindPosition),
            0x0085 => Some(Self::InfoBlindPosition),
            0x0140 => Some(Self::SetTargetTemperature),
            0x0130 => Some(Self::InfoCurrentTemperature),
            _ => None,
        }
    }

    /// The value shape this role carries.
    #[must_use]
    pub const fn value_shape(self) -> ValueShape {
        match self {
            Self::SwitchOnOff | Self::InfoOnOff | Self::MoveUpDown => ValueShape::Bool,
            Self::SetBrightness
            | Self::InfoBrightness
            | Self::SetBlindPosition
            | Self::InfoBlindPosition => ValueShape::Percent,
            Self::SetTargetTemperature | Self::InfoCurrentTemperature => {
                ValueShape::Temperature
            }
        }
    }

    /// Whether this role is a writable command target (an input datapoint).
    /// `Info…` roles are device-reported state and are read-only.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        matches!(
            self,
            Self::SwitchOnOff
                | Self::SetBrightness
                | Self::MoveUpDown
                | Self::SetBlindPosition
                | Self::SetTargetTemperature
        )
    }

    /// Check that a typed [`Value`] is acceptable for this role.
    ///
    /// # Errors
    /// Returns a [`ValueError`] describing the mismatch when the value's shape
    /// does not match the role's [`ValueShape`].
    pub const fn validate(self, value: &Value) -> Result<(), ValueError> {
        let ok = matches!(
            (self.value_shape(), value),
            (ValueShape::Bool, Value::Bool(_))
                | (ValueShape::Percent, Value::Percent(_))
                | (ValueShape::Temperature, Value::Temperature(_))
        );
        if ok {
            Ok(())
        } else {
            Err(match self.value_shape() {
                ValueShape::Bool => ValueError::NotABool,
                ValueShape::Percent => ValueError::NotAPercent,
                ValueShape::Temperature => ValueError::NotANumber,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_ids_are_stable() {
        assert_eq!(Pairing::SwitchOnOff.id(), 0x0001);
        assert_eq!(Pairing::SetBrightness.id(), 0x0011);
        assert_eq!(Pairing::MoveUpDown.id(), 0x0020);
        assert_eq!(Pairing::SetTargetTemperature.id(), 0x0140);
    }

    #[test]
    fn from_id_round_trips() {
        for p in [
            Pairing::SwitchOnOff,
            Pairing::InfoOnOff,
            Pairing::SetBrightness,
            Pairing::InfoBrightness,
            Pairing::MoveUpDown,
            Pairing::SetBlindPosition,
            Pairing::InfoBlindPosition,
            Pairing::SetTargetTemperature,
            Pairing::InfoCurrentTemperature,
        ] {
            assert_eq!(Pairing::from_id(p.id()), Some(p));
        }
        assert_eq!(Pairing::from_id(0xABCD), None);
    }

    #[test]
    fn value_shapes_are_correct() {
        assert_eq!(Pairing::SwitchOnOff.value_shape(), ValueShape::Bool);
        assert_eq!(Pairing::SetBrightness.value_shape(), ValueShape::Percent);
        assert_eq!(
            Pairing::SetTargetTemperature.value_shape(),
            ValueShape::Temperature
        );
    }

    #[test]
    fn writability_split() {
        assert!(Pairing::SwitchOnOff.is_writable());
        assert!(Pairing::SetBrightness.is_writable());
        assert!(!Pairing::InfoOnOff.is_writable());
        assert!(!Pairing::InfoCurrentTemperature.is_writable());
    }

    #[test]
    fn validate_accepts_matching_shape() {
        assert!(Pairing::SwitchOnOff.validate(&Value::Bool(true)).is_ok());
        assert!(Pairing::SetBrightness.validate(&Value::Percent(50)).is_ok());
        assert!(
            Pairing::SetTargetTemperature
                .validate(&Value::Temperature(21.0))
                .is_ok()
        );
    }

    #[test]
    fn validate_rejects_mismatched_shape() {
        assert_eq!(
            Pairing::SwitchOnOff.validate(&Value::Percent(50)),
            Err(ValueError::NotABool)
        );
        assert_eq!(
            Pairing::SetBrightness.validate(&Value::Bool(true)),
            Err(ValueError::NotAPercent)
        );
        assert_eq!(
            Pairing::SetTargetTemperature.validate(&Value::Percent(50)),
            Err(ValueError::NotANumber)
        );
    }
}
