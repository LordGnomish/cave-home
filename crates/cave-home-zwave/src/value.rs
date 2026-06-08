// SPDX-License-Identifier: Apache-2.0
//! The typed value model the Command Class decoders produce.
//!
//! The rest of cave-home does not care that a number arrived as a "Multilevel
//! Sensor Report" with a precision/scale/size byte — it cares that the bedroom
//! is 21 °C, the lamp is on, or a battery is low. [`Value`] is that vendor- and
//! protocol-neutral surface. Each Command Class decoder maps its wire form onto
//! exactly one of these variants.

/// What a temperature value is measured in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemperatureUnit {
    /// Degrees Celsius (scale 0 in the spec).
    Celsius,
    /// Degrees Fahrenheit (scale 1 in the spec).
    Fahrenheit,
}

/// A kind of measured quantity carried by a sensor value, used to pick a label
/// and unit without re-deriving it from raw protocol fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// Air or surface temperature.
    Temperature,
    /// Relative humidity, in percent.
    Humidity,
    /// Illuminance, in lux.
    Luminance,
    /// Electrical power, in watts.
    Power,
    /// Cumulative electrical energy, in kilowatt-hours.
    Energy,
    /// Any other measured number we surface generically.
    Generic,
}

/// A protocol- and vendor-neutral decoded value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// A binary on/off (a switch, a binary sensor).
    Bool(bool),

    /// A 0–100% level (a dimmer, a blind position).
    Level(u8),

    /// A temperature with its unit.
    Temperature {
        /// The measured value.
        value: f64,
        /// Celsius or Fahrenheit.
        unit: TemperatureUnit,
    },

    /// A relative humidity in percent (0–100, but not range-clamped here).
    Humidity(f64),

    /// A generic measured number tagged with what it measures.
    Measurement {
        /// The decoded value.
        value: f64,
        /// What the number measures.
        quantity: Quantity,
    },

    /// A battery charge in percent (0–100).
    BatteryPercent(u8),

    /// The battery-low sentinel — the device reports it is nearly empty rather
    /// than an exact percentage.
    BatteryLow,

    /// An RGB-style colour component (component id + 0–255 intensity).
    ColorComponent {
        /// Component id (0=warm white, 1=cold white, 2=red, 3=green, 4=blue …).
        component: u8,
        /// Component intensity, 0–255.
        intensity: u8,
    },

    /// A notification (type + event), e.g. "smoke detected".
    Notification {
        /// Notification type id (e.g. 0x01 = Smoke Alarm).
        notification_type: u8,
        /// Event id within that type.
        event: u8,
    },

    /// A configuration parameter read-back (parameter number + signed value).
    ConfigParam {
        /// Parameter number.
        parameter: u16,
        /// Signed parameter value.
        value: i32,
    },
}

impl Value {
    /// Convenience: read a boolean if this value is one.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Convenience: read a 0–100 level if this value is one.
    #[must_use]
    pub const fn as_level(&self) -> Option<u8> {
        match self {
            Self::Level(l) => Some(*l),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_narrow_correctly() {
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Level(50).as_bool(), None);
        assert_eq!(Value::Level(50).as_level(), Some(50));
        assert_eq!(Value::BatteryLow.as_level(), None);
    }

    #[test]
    fn temperature_carries_unit() {
        let v = Value::Temperature {
            value: 21.0,
            unit: TemperatureUnit::Celsius,
        };
        assert_eq!(
            v,
            Value::Temperature {
                value: 21.0,
                unit: TemperatureUnit::Celsius
            }
        );
    }
}
