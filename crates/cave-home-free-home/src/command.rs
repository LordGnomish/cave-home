// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Control commands — building and validating a datapoint write.
//!
//! A [`SetDatapoint`] is the typed, validated "turn this on / set this to N"
//! instruction. It names the device serial, the channel, the pairing *role*
//! being driven, and the typed [`Value`]. Construction validates three things
//! up front so the (deferred, Phase-1b) transport layer can write it blindly:
//!
//! 1. the role is writable (an input, not a reported-state output);
//! 2. the value's shape matches the role's [`crate::pairing::ValueShape`];
//! 3. the value itself is in range (delegated to [`Value`]).
//!
//! The transport layer is out of scope here (no network in Phase 1); a command
//! carries everything that layer needs and [`SetDatapoint::wire_value`] gives
//! the exact string to send.

use crate::id::{ChannelId, DeviceSerial, IdError};
use crate::pairing::Pairing;
use crate::value::{Value, ValueError};

/// Why a control command could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The device serial was invalid.
    BadSerial(IdError),
    /// The pairing role is read-only (a reported-state output).
    RoleNotWritable,
    /// The value was wrong for the role, or out of range.
    BadValue(ValueError),
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadSerial(e) => write!(f, "device serial invalid: {e}"),
            Self::RoleNotWritable => f.write_str("this datapoint role cannot be written"),
            Self::BadValue(e) => write!(f, "value invalid for this control: {e}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// A validated datapoint write.
#[derive(Debug, Clone, PartialEq)]
pub struct SetDatapoint {
    serial: DeviceSerial,
    channel: ChannelId,
    pairing: Pairing,
    value: Value,
}

impl SetDatapoint {
    /// Build a validated command for any pairing role + typed value.
    ///
    /// # Errors
    /// - [`CommandError::BadSerial`] if the serial does not parse;
    /// - [`CommandError::RoleNotWritable`] if the role is read-only;
    /// - [`CommandError::BadValue`] if the value shape/range is wrong.
    pub fn new(
        serial: &str,
        channel: ChannelId,
        pairing: Pairing,
        value: Value,
    ) -> Result<Self, CommandError> {
        let serial = DeviceSerial::parse(serial).map_err(CommandError::BadSerial)?;
        if !pairing.is_writable() {
            return Err(CommandError::RoleNotWritable);
        }
        pairing.validate(&value).map_err(CommandError::BadValue)?;
        Ok(Self {
            serial,
            channel,
            pairing,
            value,
        })
    }

    /// Convenience: a boolean command (switch on/off, move up/down).
    pub fn boolean(
        serial: &str,
        channel: ChannelId,
        pairing: Pairing,
        on: bool,
    ) -> Result<Self, CommandError> {
        Self::new(serial, channel, pairing, Value::Bool(on))
    }

    /// Convenience: a percentage command (brightness, blind position).
    pub fn percent(
        serial: &str,
        channel: ChannelId,
        pairing: Pairing,
        pct: u8,
    ) -> Result<Self, CommandError> {
        let value = Value::percent(pct).map_err(CommandError::BadValue)?;
        Self::new(serial, channel, pairing, value)
    }

    /// Convenience: a temperature setpoint command.
    pub fn temperature(
        serial: &str,
        channel: ChannelId,
        pairing: Pairing,
        celsius: f64,
    ) -> Result<Self, CommandError> {
        let value = Value::temperature(celsius).map_err(CommandError::BadValue)?;
        Self::new(serial, channel, pairing, value)
    }

    #[must_use]
    pub const fn serial(&self) -> &DeviceSerial {
        &self.serial
    }

    #[must_use]
    pub const fn channel(&self) -> ChannelId {
        self.channel
    }

    #[must_use]
    pub const fn pairing(&self) -> Pairing {
        self.pairing
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    /// The exact string the (deferred) transport must write for this command.
    #[must_use]
    pub fn wire_value(&self) -> String {
        self.value.encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERIAL: &str = "ABB700C12345";

    fn ch() -> ChannelId {
        ChannelId::new(3)
    }

    #[test]
    fn switch_on_builds() {
        let cmd = SetDatapoint::boolean(SERIAL, ch(), Pairing::SwitchOnOff, true)
            .expect("valid");
        assert_eq!(cmd.wire_value(), "1");
        assert_eq!(cmd.pairing(), Pairing::SwitchOnOff);
        assert_eq!(cmd.channel().index(), 3);
        assert_eq!(cmd.serial().as_str(), SERIAL);
    }

    #[test]
    fn dimmer_percent_builds() {
        let cmd =
            SetDatapoint::percent(SERIAL, ch(), Pairing::SetBrightness, 50).expect("valid");
        assert_eq!(cmd.wire_value(), "50");
    }

    #[test]
    fn blind_position_builds() {
        let cmd =
            SetDatapoint::percent(SERIAL, ch(), Pairing::SetBlindPosition, 75).expect("valid");
        assert_eq!(cmd.wire_value(), "75");
    }

    #[test]
    fn thermostat_target_builds() {
        let cmd = SetDatapoint::temperature(SERIAL, ch(), Pairing::SetTargetTemperature, 21.5)
            .expect("valid");
        assert_eq!(cmd.wire_value(), "21.5");
    }

    #[test]
    fn rejects_read_only_role() {
        let r = SetDatapoint::boolean(SERIAL, ch(), Pairing::InfoOnOff, true);
        assert_eq!(r, Err(CommandError::RoleNotWritable));
    }

    #[test]
    fn rejects_wrong_value_shape() {
        // A boolean into a brightness control is rejected.
        let r = SetDatapoint::new(SERIAL, ch(), Pairing::SetBrightness, Value::Bool(true));
        assert_eq!(r, Err(CommandError::BadValue(ValueError::NotAPercent)));
    }

    #[test]
    fn rejects_out_of_range_percent() {
        let r = SetDatapoint::percent(SERIAL, ch(), Pairing::SetBrightness, 200);
        assert_eq!(
            r,
            Err(CommandError::BadValue(ValueError::PercentOutOfRange))
        );
    }

    #[test]
    fn rejects_bad_serial() {
        let r = SetDatapoint::boolean("bad serial!", ch(), Pairing::SwitchOnOff, true);
        assert!(matches!(r, Err(CommandError::BadSerial(_))));
    }

    #[test]
    fn rejects_non_finite_temperature() {
        let r = SetDatapoint::temperature(SERIAL, ch(), Pairing::SetTargetTemperature, f64::NAN);
        assert_eq!(
            r,
            Err(CommandError::BadValue(ValueError::TemperatureNotFinite))
        );
    }
}
