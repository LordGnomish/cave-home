// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The datapoint value codec.
//!
//! On the wire, every free@home datapoint value is a **string**. A boolean is
//! `"0"` / `"1"`; a percentage is `"0"`..`"100"`; a temperature is a decimal
//! float rendered as text (`"21.5"`). This module is the typed boundary: a
//! [`Value`] decodes from, and encodes back to, that wire string — with bounds
//! checking and the rounding the System Access Point expects (percentages are
//! integers; temperatures are emitted to one decimal place).

use core::fmt;

/// A typed datapoint value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A boolean (`"0"` / `"1"`).
    Bool(bool),
    /// A percentage clamped to `0..=100`.
    Percent(u8),
    /// A temperature in degrees Celsius.
    Temperature(f64),
}

/// Why a wire value failed to decode, or a typed value failed to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// The wire string was not a recognised boolean (`"0"`/`"1"`).
    NotABool,
    /// The wire string was not a valid integer percentage.
    NotAPercent,
    /// A percentage was outside `0..=100`.
    PercentOutOfRange,
    /// The wire string was not a valid number.
    NotANumber,
    /// A temperature was not finite (`NaN` / infinite).
    TemperatureNotFinite,
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotABool => f.write_str("value is not 0 or 1"),
            Self::NotAPercent => f.write_str("value is not an integer percentage"),
            Self::PercentOutOfRange => f.write_str("percentage is outside 0..=100"),
            Self::NotANumber => f.write_str("value is not a number"),
            Self::TemperatureNotFinite => f.write_str("temperature is not finite"),
        }
    }
}

impl std::error::Error for ValueError {}

impl Value {
    /// Construct a percentage, rejecting values above 100.
    pub const fn percent(p: u8) -> Result<Self, ValueError> {
        if p > 100 {
            return Err(ValueError::PercentOutOfRange);
        }
        Ok(Self::Percent(p))
    }

    /// Construct a temperature, rejecting non-finite values.
    pub const fn temperature(c: f64) -> Result<Self, ValueError> {
        if !c.is_finite() {
            return Err(ValueError::TemperatureNotFinite);
        }
        Ok(Self::Temperature(c))
    }

    /// Decode a boolean from its wire string.
    pub fn decode_bool(wire: &str) -> Result<Self, ValueError> {
        match wire.trim() {
            "0" => Ok(Self::Bool(false)),
            "1" => Ok(Self::Bool(true)),
            _ => Err(ValueError::NotABool),
        }
    }

    /// Decode a percentage from its wire string. The System Access Point may
    /// send a decimal (`"50.0"`); we round to the nearest integer percent and
    /// bound-check.
    pub fn decode_percent(wire: &str) -> Result<Self, ValueError> {
        let raw: f64 = wire.trim().parse().map_err(|_| ValueError::NotAPercent)?;
        if !raw.is_finite() {
            return Err(ValueError::NotAPercent);
        }
        let rounded = raw.round();
        if !(0.0..=100.0).contains(&rounded) {
            return Err(ValueError::PercentOutOfRange);
        }
        // `rounded` is an integer-valued f64 bounded to 0..=100, so the cast is
        // exact and neither truncates nor loses sign.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct = rounded as u8;
        Ok(Self::Percent(pct))
    }

    /// Decode a temperature from its wire string.
    pub fn decode_temperature(wire: &str) -> Result<Self, ValueError> {
        let raw: f64 = wire.trim().parse().map_err(|_| ValueError::NotANumber)?;
        Self::temperature(raw)
    }

    /// Encode this value to its wire string.
    ///
    /// - `Bool` → `"0"` / `"1"`
    /// - `Percent` → the integer (`"50"`)
    /// - `Temperature` → one decimal place (`"21.5"`)
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Bool(true) => "1".to_string(),
            Self::Bool(false) => "0".to_string(),
            Self::Percent(p) => p.to_string(),
            Self::Temperature(c) => format!("{c:.1}"),
        }
    }

    /// The boolean payload, if this is a `Bool`.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The percentage payload, if this is a `Percent`.
    #[must_use]
    pub const fn as_percent(&self) -> Option<u8> {
        match self {
            Self::Percent(p) => Some(*p),
            _ => None,
        }
    }

    /// The temperature payload, if this is a `Temperature`.
    #[must_use]
    pub const fn as_temperature(&self) -> Option<f64> {
        match self {
            Self::Temperature(c) => Some(*c),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_decode_round_trip() {
        assert_eq!(Value::decode_bool("0"), Ok(Value::Bool(false)));
        assert_eq!(Value::decode_bool("1"), Ok(Value::Bool(true)));
        assert_eq!(Value::decode_bool(" 1 "), Ok(Value::Bool(true)));
        assert_eq!(Value::Bool(true).encode(), "1");
        assert_eq!(Value::Bool(false).encode(), "0");
    }

    #[test]
    fn bool_decode_rejects_garbage() {
        assert_eq!(Value::decode_bool("2"), Err(ValueError::NotABool));
        assert_eq!(Value::decode_bool("true"), Err(ValueError::NotABool));
        assert_eq!(Value::decode_bool(""), Err(ValueError::NotABool));
    }

    #[test]
    fn percent_decode_round_trip() {
        assert_eq!(Value::decode_percent("0"), Ok(Value::Percent(0)));
        assert_eq!(Value::decode_percent("50"), Ok(Value::Percent(50)));
        assert_eq!(Value::decode_percent("100"), Ok(Value::Percent(100)));
        assert_eq!(Value::Percent(50).encode(), "50");
    }

    #[test]
    fn percent_decode_rounds_decimals() {
        assert_eq!(Value::decode_percent("49.4"), Ok(Value::Percent(49)));
        assert_eq!(Value::decode_percent("49.5"), Ok(Value::Percent(50)));
        assert_eq!(Value::decode_percent("0.4"), Ok(Value::Percent(0)));
        assert_eq!(Value::decode_percent("99.6"), Ok(Value::Percent(100)));
    }

    #[test]
    fn percent_bounds_enforced() {
        assert_eq!(Value::percent(100), Ok(Value::Percent(100)));
        assert_eq!(Value::percent(101), Err(ValueError::PercentOutOfRange));
        assert_eq!(
            Value::decode_percent("100.6"),
            Err(ValueError::PercentOutOfRange)
        );
        assert_eq!(
            Value::decode_percent("-1"),
            Err(ValueError::PercentOutOfRange)
        );
        assert_eq!(Value::decode_percent("nope"), Err(ValueError::NotAPercent));
    }

    #[test]
    fn temperature_decode_round_trip_one_decimal() {
        assert_eq!(
            Value::decode_temperature("21.5"),
            Ok(Value::Temperature(21.5))
        );
        assert_eq!(Value::Temperature(21.5).encode(), "21.5");
        // Encoding renders exactly one decimal place.
        assert_eq!(Value::Temperature(20.0).encode(), "20.0");
        assert_eq!(Value::Temperature(19.04).encode(), "19.0");
        assert_eq!(Value::Temperature(19.06).encode(), "19.1");
    }

    #[test]
    fn temperature_rejects_non_finite() {
        assert_eq!(
            Value::temperature(f64::NAN),
            Err(ValueError::TemperatureNotFinite)
        );
        assert_eq!(
            Value::decode_temperature("nan").map(|_| ()).unwrap_err(),
            ValueError::TemperatureNotFinite
        );
        assert_eq!(
            Value::decode_temperature("hot"),
            Err(ValueError::NotANumber)
        );
    }

    #[test]
    fn accessors_match_variant() {
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Bool(true).as_percent(), None);
        assert_eq!(Value::Percent(40).as_percent(), Some(40));
        assert_eq!(Value::Temperature(5.0).as_temperature(), Some(5.0));
        assert_eq!(Value::Temperature(5.0).as_bool(), None);
    }
}
