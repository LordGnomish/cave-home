// SPDX-License-Identifier: Apache-2.0
//! Command Class framing + the typed command model.
//!
//! A Z-Wave application payload is a flat byte string. The first octet is the
//! **Command Class** id, the second is the **command** id within that class,
//! and the remainder is the command-specific body. This module models that:
//!
//! - [`CommandClass`] — the Command Class id enum (with `from_u8`/`to_u8`).
//! - [`Command`] — one decoded, typed command across the Command Classes
//!   cave-home ships in Phase 1.
//! - [`Command::decode`] / [`Command::encode`] — the round-trip between raw
//!   payload bytes and the typed form.
//!
//! Every decoder rejects truncated or out-of-range payloads via
//! [`crate::error::ZwaveError`]; nothing here panics on malformed input.
//!
//! Implemented from the **public** Silicon Labs Z-Wave Command Class
//! specification (SDS13781 family). This is a clean-room first-party
//! implementation of public protocol behavior.

// The Configuration/Color encoders pack signed parameter values into a chosen
// 1/2/4-octet width; those casts are deliberate width-narrowing of values the
// caller is responsible for, so the wrap/truncation lints are silenced here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use crate::error::{ZwaveError, ZwaveResult};
use crate::sensor_decode::{self, FixedPoint};
use crate::value::{Quantity, TemperatureUnit, Value};

/// The Command Class identifiers cave-home models in Phase 1.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CommandClass {
    /// Basic CC (0x20) — the lowest-common-denominator on/off/level.
    Basic,
    /// Binary Switch CC (0x25) — on/off actuators.
    BinarySwitch,
    /// Multilevel Switch CC (0x26) — dimmers / motors with a 0–99 level.
    MultilevelSwitch,
    /// Binary Sensor CC (0x30) — yes/no sensors.
    BinarySensor,
    /// Multilevel Sensor CC (0x31) — measured numbers (temperature, …).
    MultilevelSensor,
    /// Configuration CC (0x70) — device parameters.
    Configuration,
    /// Notification CC (0x71) — event reports (smoke, motion, …).
    Notification,
    /// Battery CC (0x80) — battery level.
    Battery,
    /// Color Switch CC (0x33) — multi-component colour lights.
    ColorSwitch,
    /// Meter CC (0x32) — accumulated energy / power meters.
    Meter,
    /// Thermostat Setpoint CC (0x43) — target temperatures.
    ThermostatSetpoint,
    /// Thermostat Mode CC (0x40) — heating/cooling mode selection.
    ThermostatMode,
}

impl CommandClass {
    /// Map an id byte to a [`CommandClass`], if cave-home models it.
    #[must_use]
    pub const fn from_u8(id: u8) -> Option<Self> {
        match id {
            0x20 => Some(Self::Basic),
            0x25 => Some(Self::BinarySwitch),
            0x26 => Some(Self::MultilevelSwitch),
            0x30 => Some(Self::BinarySensor),
            0x31 => Some(Self::MultilevelSensor),
            0x32 => Some(Self::Meter),
            0x33 => Some(Self::ColorSwitch),
            0x40 => Some(Self::ThermostatMode),
            0x43 => Some(Self::ThermostatSetpoint),
            0x70 => Some(Self::Configuration),
            0x71 => Some(Self::Notification),
            0x80 => Some(Self::Battery),
            _ => None,
        }
    }

    /// The id byte for this Command Class.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Basic => 0x20,
            Self::BinarySwitch => 0x25,
            Self::MultilevelSwitch => 0x26,
            Self::BinarySensor => 0x30,
            Self::MultilevelSensor => 0x31,
            Self::Meter => 0x32,
            Self::ColorSwitch => 0x33,
            Self::ThermostatMode => 0x40,
            Self::ThermostatSetpoint => 0x43,
            Self::Configuration => 0x70,
            Self::Notification => 0x71,
            Self::Battery => 0x80,
        }
    }
}

/// The direction a Multilevel Switch level change runs in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelChange {
    /// Brightening / opening.
    Up,
    /// Dimming / closing.
    Down,
}

/// A Thermostat Mode CC (0x40) operating mode.
///
/// The wire byte carries the mode in its low 5 bits. cave-home models the
/// common named modes plus the `ManufacturerSpecific` (0x1F) sentinel; any
/// other (unassigned) byte is rejected as out-of-range rather than fabricated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThermostatMode {
    /// Heating and cooling both off.
    Off,
    /// Heat to the heating setpoint.
    Heat,
    /// Cool to the cooling setpoint.
    Cool,
    /// Automatically heat or cool to maintain a target.
    Auto,
    /// Energy-saving heating mode.
    EnergySaveHeat,
    /// Energy-saving cooling mode.
    EnergySaveCool,
    /// A manufacturer-defined mode (0x1F sentinel).
    ManufacturerSpecific,
}

impl ThermostatMode {
    /// Map a mode byte to a [`ThermostatMode`], if cave-home models it.
    #[must_use]
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Off),
            0x01 => Some(Self::Heat),
            0x02 => Some(Self::Cool),
            0x03 => Some(Self::Auto),
            0x0B => Some(Self::EnergySaveHeat),
            0x0C => Some(Self::EnergySaveCool),
            0x1F => Some(Self::ManufacturerSpecific),
            _ => None,
        }
    }

    /// The mode byte for this [`ThermostatMode`].
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::Heat => 0x01,
            Self::Cool => 0x02,
            Self::Auto => 0x03,
            Self::EnergySaveHeat => 0x0B,
            Self::EnergySaveCool => 0x0C,
            Self::ManufacturerSpecific => 0x1F,
        }
    }
}

/// One decoded, typed Command Class command.
///
/// Variants are grouped by Command Class. `Get` commands carry no body; `Set`
/// commands carry the value to apply; `Report` commands carry the value the
/// device announced.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Command {
    // ---- Basic CC (0x20) ----------------------------------------------------
    /// Set the device to a Basic value (0–99 = level %, 0xFF = "on/last").
    BasicSet(u8),
    /// Ask for the current Basic value.
    BasicGet,
    /// The device's current Basic value.
    BasicReport(u8),

    // ---- Binary Switch CC (0x25) -------------------------------------------
    /// Turn a switch on (`true`) or off (`false`).
    BinarySwitchSet(bool),
    /// Ask whether a switch is on.
    BinarySwitchGet,
    /// The switch's current on/off state.
    BinarySwitchReport(bool),

    // ---- Multilevel Switch CC (0x26) ---------------------------------------
    /// Set a dimmer/motor to a level (0–99, or 0xFF = "restore last").
    MultilevelSwitchSet(u8),
    /// Ask for the current level.
    MultilevelSwitchGet,
    /// The current level (0–99, or 0xFF).
    MultilevelSwitchReport(u8),
    /// Begin a continuous level change.
    MultilevelSwitchStartLevelChange(LevelChange),
    /// Stop a continuous level change.
    MultilevelSwitchStopLevelChange,

    // ---- Binary Sensor CC (0x30) -------------------------------------------
    /// Ask for a binary sensor's state.
    BinarySensorGet,
    /// A binary sensor's current state (triggered = `true`).
    BinarySensorReport(bool),

    // ---- Multilevel Sensor CC (0x31) ---------------------------------------
    /// Ask for a multilevel sensor reading.
    MultilevelSensorGet,
    /// A multilevel sensor reading (sensor type + decoded fixed-point value).
    MultilevelSensorReport {
        /// Sensor type id (e.g. 0x01 = air temperature, 0x05 = humidity).
        sensor_type: u8,
        /// The decoded value, scale and precision.
        reading: FixedPoint,
    },

    // ---- Meter CC (0x32) ---------------------------------------------------
    /// Ask for a meter reading.
    MeterGet,
    /// A meter reading (meter type + decoded fixed-point value).
    MeterReport {
        /// Meter type id (low 5 bits: 1=electric, 2=gas, 3=water …).
        meter_type: u8,
        /// The decoded value, scale and precision.
        reading: FixedPoint,
    },

    // ---- Color Switch CC (0x33) --------------------------------------------
    /// Set one colour component (component id + 0–255 intensity).
    ColorSwitchSet {
        /// Component id.
        component: u8,
        /// Intensity 0–255.
        value: u8,
    },
    /// Ask for one colour component's value.
    ColorSwitchGet {
        /// Component id to query.
        component: u8,
    },
    /// A colour component's current value.
    ColorSwitchReport {
        /// Component id.
        component: u8,
        /// Intensity 0–255.
        value: u8,
    },

    // ---- Thermostat Setpoint CC (0x43) -------------------------------------
    /// Set a thermostat setpoint (setpoint type + temperature value).
    ThermostatSetpointSet {
        /// Setpoint type id (1=heating, 2=cooling …).
        setpoint_type: u8,
        /// The decoded value, scale and precision.
        value: FixedPoint,
    },
    /// Ask for a setpoint.
    ThermostatSetpointGet {
        /// Setpoint type id.
        setpoint_type: u8,
    },
    /// A setpoint's current value.
    ThermostatSetpointReport {
        /// Setpoint type id.
        setpoint_type: u8,
        /// The decoded value, scale and precision.
        value: FixedPoint,
    },

    // ---- Thermostat Mode CC (0x40) -----------------------------------------
    /// Set the thermostat operating mode.
    ThermostatModeSet(ThermostatMode),
    /// Ask for the current thermostat mode.
    ThermostatModeGet,
    /// The thermostat's current operating mode.
    ThermostatModeReport(ThermostatMode),

    // ---- Configuration CC (0x70) -------------------------------------------
    /// Set a configuration parameter.
    ConfigurationSet {
        /// Parameter number.
        parameter: u8,
        /// Size in octets (1/2/4).
        size: u8,
        /// Signed value to write.
        value: i32,
    },
    /// Ask for a configuration parameter.
    ConfigurationGet {
        /// Parameter number.
        parameter: u8,
    },
    /// A configuration parameter read-back.
    ConfigurationReport {
        /// Parameter number.
        parameter: u8,
        /// Size in octets (1/2/4).
        size: u8,
        /// Signed value.
        value: i32,
    },

    // ---- Notification CC (0x71) --------------------------------------------
    /// A notification event report.
    NotificationReport {
        /// Notification type id (e.g. 0x01 = Smoke Alarm).
        notification_type: u8,
        /// Event id within that type.
        event: u8,
    },

    // ---- Battery CC (0x80) -------------------------------------------------
    /// Ask for a battery level.
    BatteryGet,
    /// A battery level (0–100%, or the 0xFF "low battery" sentinel).
    BatteryReport(u8),
}

// Command id constants, grouped by Command Class, for readability.
mod cmd {
    // Common Set/Get/Report across the simple actuator/sensor classes.
    pub const SET: u8 = 0x01;
    pub const GET: u8 = 0x02;
    pub const REPORT: u8 = 0x03;
    // Multilevel Switch extras.
    pub const START_LEVEL_CHANGE: u8 = 0x04;
    pub const STOP_LEVEL_CHANGE: u8 = 0x05;
    // Notification report command id.
    pub const NOTIFICATION_REPORT: u8 = 0x05;
}

/// The Basic/Multilevel-Switch sentinel meaning "on / restore previous level".
pub const SWITCH_ON: u8 = 0xFF;
/// The Battery CC sentinel meaning "battery low".
pub const BATTERY_LOW: u8 = 0xFF;

impl Command {
    /// Decode a raw Command Class payload into a typed [`Command`].
    ///
    /// `payload[0]` is the Command Class id, `payload[1]` the command id, and
    /// the rest the command body.
    ///
    /// # Errors
    /// - [`ZwaveError::Truncated`] if the payload is too short.
    /// - [`ZwaveError::UnknownCommand`] for a command id we do not model.
    /// - [`ZwaveError::OutOfRange`] / [`ZwaveError::BadValueSize`] for bad fields.
    pub fn decode(payload: &[u8]) -> ZwaveResult<Self> {
        if payload.len() < 2 {
            return Err(ZwaveError::Truncated {
                need: 2,
                got: payload.len(),
            });
        }
        let cc_id = payload[0];
        let cmd = payload[1];
        let body = &payload[2..];
        let cc = CommandClass::from_u8(cc_id)
            .ok_or(ZwaveError::UnknownCommand { command_class: cc_id, command: cmd })?;
        match cc {
            CommandClass::Basic => Self::decode_basic(cmd, body),
            CommandClass::BinarySwitch => Self::decode_binary_switch(cmd, body),
            CommandClass::MultilevelSwitch => Self::decode_multilevel_switch(cmd, body),
            CommandClass::BinarySensor => Self::decode_binary_sensor(cmd, body),
            CommandClass::MultilevelSensor => Self::decode_multilevel_sensor(cmd, body),
            CommandClass::Meter => Self::decode_meter(cmd, body),
            CommandClass::ColorSwitch => Self::decode_color_switch(cmd, body),
            CommandClass::ThermostatMode => Self::decode_thermostat_mode(cmd, body),
            CommandClass::ThermostatSetpoint => Self::decode_thermostat_setpoint(cmd, body),
            CommandClass::Configuration => Self::decode_configuration(cmd, body),
            CommandClass::Notification => Self::decode_notification(cmd, body),
            CommandClass::Battery => Self::decode_battery(cmd, body),
        }
    }

    /// Encode a typed [`Command`] back into a raw Command Class payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            // Basic CC.
            Self::BasicSet(v) => vec![0x20, cmd::SET, *v],
            Self::BasicGet => vec![0x20, cmd::GET],
            Self::BasicReport(v) => vec![0x20, cmd::REPORT, *v],

            // Binary Switch CC.
            Self::BinarySwitchSet(on) => vec![0x25, cmd::SET, bool_to_switch(*on)],
            Self::BinarySwitchGet => vec![0x25, cmd::GET],
            Self::BinarySwitchReport(on) => vec![0x25, cmd::REPORT, bool_to_switch(*on)],

            // Multilevel Switch CC.
            Self::MultilevelSwitchSet(v) => vec![0x26, cmd::SET, *v],
            Self::MultilevelSwitchGet => vec![0x26, cmd::GET],
            Self::MultilevelSwitchReport(v) => vec![0x26, cmd::REPORT, *v],
            Self::MultilevelSwitchStartLevelChange(dir) => {
                // Bit 6 of the control byte selects the direction (1 = down).
                let control = match dir {
                    LevelChange::Up => 0x00,
                    LevelChange::Down => 0x40,
                };
                // start level present-flag clear; a 0 start level placeholder.
                vec![0x26, cmd::START_LEVEL_CHANGE, control, 0x00]
            }
            Self::MultilevelSwitchStopLevelChange => vec![0x26, cmd::STOP_LEVEL_CHANGE],

            // Binary Sensor CC.
            Self::BinarySensorGet => vec![0x30, cmd::GET],
            Self::BinarySensorReport(on) => vec![0x30, cmd::REPORT, bool_to_switch(*on)],

            // Multilevel Sensor CC.
            Self::MultilevelSensorGet => vec![0x31, cmd::GET],
            Self::MultilevelSensorReport { sensor_type, reading } => {
                let mut out = vec![0x31, cmd::REPORT, *sensor_type];
                push_fixed(&mut out, reading);
                out
            }

            // Meter CC.
            Self::MeterGet => vec![0x32, cmd::GET],
            Self::MeterReport { meter_type, reading } => {
                let mut out = vec![0x32, cmd::REPORT, *meter_type];
                push_fixed(&mut out, reading);
                out
            }

            // Color Switch CC.
            Self::ColorSwitchSet { component, value } => {
                vec![0x33, cmd::SET, *component, *value]
            }
            Self::ColorSwitchGet { component } => vec![0x33, cmd::GET, *component],
            Self::ColorSwitchReport { component, value } => {
                vec![0x33, cmd::REPORT, *component, *value]
            }

            // Thermostat Setpoint CC.
            Self::ThermostatSetpointSet { setpoint_type, value } => {
                let mut out = vec![0x43, cmd::SET, *setpoint_type];
                push_fixed(&mut out, value);
                out
            }
            Self::ThermostatSetpointGet { setpoint_type } => {
                vec![0x43, cmd::GET, *setpoint_type]
            }
            Self::ThermostatSetpointReport { setpoint_type, value } => {
                let mut out = vec![0x43, cmd::REPORT, *setpoint_type];
                push_fixed(&mut out, value);
                out
            }

            // Thermostat Mode CC.
            Self::ThermostatModeSet(mode) => vec![0x40, cmd::SET, mode.to_u8()],
            Self::ThermostatModeGet => vec![0x40, cmd::GET],
            Self::ThermostatModeReport(mode) => vec![0x40, cmd::REPORT, mode.to_u8()],

            // Configuration CC.
            Self::ConfigurationSet { parameter, size, value } => {
                let mut out = vec![0x70, cmd::SET, *parameter, *size];
                push_signed(&mut out, *value, *size);
                out
            }
            Self::ConfigurationGet { parameter } => vec![0x70, cmd::GET, *parameter],
            Self::ConfigurationReport { parameter, size, value } => {
                let mut out = vec![0x70, cmd::REPORT, *parameter, *size];
                push_signed(&mut out, *value, *size);
                out
            }

            // Notification CC.
            Self::NotificationReport { notification_type, event } => {
                // v2+ report: reserved(0), reserved(0), type, status(0xFF), event…
                vec![
                    0x71,
                    cmd::NOTIFICATION_REPORT,
                    0x00,
                    0x00,
                    0x00,
                    *notification_type,
                    0xFF,
                    *event,
                ]
            }

            // Battery CC.
            Self::BatteryGet => vec![0x80, cmd::GET],
            Self::BatteryReport(level) => vec![0x80, cmd::REPORT, *level],
        }
    }

    /// Project this command onto the protocol-neutral [`Value`] model, if it
    /// carries a value worth surfacing. `Get` commands return `None`.
    #[must_use]
    pub fn to_value(&self) -> Option<Value> {
        match self {
            Self::BasicSet(v) | Self::BasicReport(v) | Self::MultilevelSwitchSet(v)
            | Self::MultilevelSwitchReport(v) => Some(level_value(*v)),

            Self::BinarySwitchSet(b)
            | Self::BinarySwitchReport(b)
            | Self::BinarySensorReport(b) => Some(Value::Bool(*b)),

            Self::MultilevelSensorReport { sensor_type, reading } => {
                Some(sensor_value(*sensor_type, reading))
            }

            Self::MeterReport { reading, .. } => Some(Value::Measurement {
                value: reading.value,
                quantity: if reading.scale == 0 { Quantity::Energy } else { Quantity::Power },
            }),

            Self::ThermostatSetpointSet { value, .. }
            | Self::ThermostatSetpointReport { value, .. } => Some(temperature_value(value)),

            Self::ColorSwitchSet { component, value }
            | Self::ColorSwitchReport { component, value } => Some(Value::ColorComponent {
                component: *component,
                intensity: *value,
            }),

            Self::NotificationReport { notification_type, event } => Some(Value::Notification {
                notification_type: *notification_type,
                event: *event,
            }),

            Self::ConfigurationSet { parameter, value, .. }
            | Self::ConfigurationReport { parameter, value, .. } => Some(Value::ConfigParam {
                parameter: u16::from(*parameter),
                value: *value,
            }),

            Self::BatteryReport(level) => Some(if *level == BATTERY_LOW {
                Value::BatteryLow
            } else {
                Value::BatteryPercent(*level)
            }),

            _ => None,
        }
    }

    // ---- per-Command-Class decoders ----------------------------------------

    fn decode_basic(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => Ok(Self::BasicSet(byte0(body, "basic_value")?)),
            cmd::GET => Ok(Self::BasicGet),
            cmd::REPORT => Ok(Self::BasicReport(byte0(body, "basic_value")?)),
            _ => Err(unknown(0x20, cmd)),
        }
    }

    fn decode_binary_switch(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => Ok(Self::BinarySwitchSet(switch_to_bool(byte0(body, "switch")?))),
            cmd::GET => Ok(Self::BinarySwitchGet),
            cmd::REPORT => Ok(Self::BinarySwitchReport(switch_to_bool(byte0(body, "switch")?))),
            _ => Err(unknown(0x25, cmd)),
        }
    }

    fn decode_multilevel_switch(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => Ok(Self::MultilevelSwitchSet(level_or_on(byte0(body, "level")?)?)),
            cmd::GET => Ok(Self::MultilevelSwitchGet),
            cmd::REPORT => Ok(Self::MultilevelSwitchReport(level_or_on(byte0(body, "level")?)?)),
            cmd::START_LEVEL_CHANGE => {
                let control = byte0(body, "control")?;
                let dir = if control & 0x40 == 0 {
                    LevelChange::Up
                } else {
                    LevelChange::Down
                };
                Ok(Self::MultilevelSwitchStartLevelChange(dir))
            }
            cmd::STOP_LEVEL_CHANGE => Ok(Self::MultilevelSwitchStopLevelChange),
            _ => Err(unknown(0x26, cmd)),
        }
    }

    fn decode_binary_sensor(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::GET => Ok(Self::BinarySensorGet),
            cmd::REPORT => Ok(Self::BinarySensorReport(switch_to_bool(byte0(body, "sensor")?))),
            _ => Err(unknown(0x30, cmd)),
        }
    }

    fn decode_multilevel_sensor(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::GET => Ok(Self::MultilevelSensorGet),
            cmd::REPORT => {
                let sensor_type = byte0(body, "sensor_type")?;
                let reading = sensor_decode::decode(&body[1..])?;
                Ok(Self::MultilevelSensorReport { sensor_type, reading })
            }
            _ => Err(unknown(0x31, cmd)),
        }
    }

    fn decode_meter(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::GET => Ok(Self::MeterGet),
            cmd::REPORT => {
                let meter_type = byte0(body, "meter_type")?;
                let reading = sensor_decode::decode(&body[1..])?;
                Ok(Self::MeterReport { meter_type, reading })
            }
            _ => Err(unknown(0x32, cmd)),
        }
    }

    fn decode_color_switch(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => {
                need(body, 2)?;
                Ok(Self::ColorSwitchSet { component: body[0], value: body[1] })
            }
            cmd::GET => Ok(Self::ColorSwitchGet { component: byte0(body, "component")? }),
            cmd::REPORT => {
                need(body, 2)?;
                Ok(Self::ColorSwitchReport { component: body[0], value: body[1] })
            }
            _ => Err(unknown(0x33, cmd)),
        }
    }

    fn decode_thermostat_mode(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => Ok(Self::ThermostatModeSet(mode_from_byte(byte0(body, "thermostat_mode")?)?)),
            cmd::GET => Ok(Self::ThermostatModeGet),
            cmd::REPORT => {
                Ok(Self::ThermostatModeReport(mode_from_byte(byte0(body, "thermostat_mode")?)?))
            }
            _ => Err(unknown(0x40, cmd)),
        }
    }

    fn decode_thermostat_setpoint(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => {
                let setpoint_type = byte0(body, "setpoint_type")?;
                let value = sensor_decode::decode(&body[1..])?;
                Ok(Self::ThermostatSetpointSet { setpoint_type, value })
            }
            cmd::GET => Ok(Self::ThermostatSetpointGet { setpoint_type: byte0(body, "setpoint_type")? }),
            cmd::REPORT => {
                let setpoint_type = byte0(body, "setpoint_type")?;
                let value = sensor_decode::decode(&body[1..])?;
                Ok(Self::ThermostatSetpointReport { setpoint_type, value })
            }
            _ => Err(unknown(0x43, cmd)),
        }
    }

    fn decode_configuration(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::SET => {
                need(body, 2)?;
                let parameter = body[0];
                let size = body[1] & 0x07;
                let value = read_signed(&body[2..], size)?;
                Ok(Self::ConfigurationSet { parameter, size, value })
            }
            cmd::GET => Ok(Self::ConfigurationGet { parameter: byte0(body, "parameter")? }),
            cmd::REPORT => {
                need(body, 2)?;
                let parameter = body[0];
                let size = body[1] & 0x07;
                let value = read_signed(&body[2..], size)?;
                Ok(Self::ConfigurationReport { parameter, size, value })
            }
            _ => Err(unknown(0x70, cmd)),
        }
    }

    fn decode_notification(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            // v2+ Notification Report layout:
            //   [0..2] V1 alarm type/level (legacy, 0 in v2), [2] reserved,
            //   [3] notification type, [4] notification status, [5] event …
            cmd::NOTIFICATION_REPORT => {
                need(body, 6)?;
                Ok(Self::NotificationReport {
                    notification_type: body[3],
                    event: body[5],
                })
            }
            _ => Err(unknown(0x71, cmd)),
        }
    }

    fn decode_battery(cmd: u8, body: &[u8]) -> ZwaveResult<Self> {
        match cmd {
            cmd::GET => Ok(Self::BatteryGet),
            cmd::REPORT => {
                let level = byte0(body, "battery_level")?;
                if level != BATTERY_LOW && level > 100 {
                    return Err(ZwaveError::OutOfRange {
                        field: "battery_level",
                        value: u32::from(level),
                    });
                }
                Ok(Self::BatteryReport(level))
            }
            _ => Err(unknown(0x80, cmd)),
        }
    }
}

// ---- shared helpers --------------------------------------------------------

const fn unknown(command_class: u8, command: u8) -> ZwaveError {
    ZwaveError::UnknownCommand { command_class, command }
}

const fn need(body: &[u8], n: usize) -> ZwaveResult<()> {
    if body.len() < n {
        // +2 accounts for the cc/cmd header already consumed.
        Err(ZwaveError::Truncated { need: n + 2, got: body.len() + 2 })
    } else {
        Ok(())
    }
}

fn byte0(body: &[u8], _field: &'static str) -> ZwaveResult<u8> {
    body.first()
        .copied()
        .ok_or(ZwaveError::Truncated { need: 3, got: 2 })
}

/// Map a Thermostat Mode byte to a modelled [`ThermostatMode`], rejecting
/// unassigned values with `OutOfRange` per the crate's discrete-value convention.
fn mode_from_byte(b: u8) -> ZwaveResult<ThermostatMode> {
    ThermostatMode::from_u8(b)
        .ok_or_else(|| ZwaveError::OutOfRange { field: "thermostat_mode", value: u32::from(b) })
}

const fn bool_to_switch(on: bool) -> u8 {
    if on { 0xFF } else { 0x00 }
}

const fn switch_to_bool(b: u8) -> bool {
    b != 0x00
}

/// Validate a Multilevel Switch level: 0..=99 or the 0xFF "on" sentinel. The
/// spec reserves 100..=254.
fn level_or_on(v: u8) -> ZwaveResult<u8> {
    if v <= 99 || v == SWITCH_ON {
        Ok(v)
    } else {
        Err(ZwaveError::OutOfRange { field: "level", value: u32::from(v) })
    }
}

const fn level_value(v: u8) -> Value {
    if v == SWITCH_ON {
        // "on / restore" — surface as a fully-on level.
        Value::Level(99)
    } else if v <= 99 {
        Value::Level(v)
    } else {
        // Reserved range; surface as off rather than fabricate a level.
        Value::Level(0)
    }
}

const fn sensor_value(sensor_type: u8, fp: &FixedPoint) -> Value {
    match sensor_type {
        // 0x01 = Air temperature.
        0x01 => temperature_value(fp),
        // 0x05 = Relative humidity.
        0x05 => Value::Humidity(fp.value),
        // 0x03 = Luminance.
        0x03 => Value::Measurement { value: fp.value, quantity: Quantity::Luminance },
        // 0x04 = Power.
        0x04 => Value::Measurement { value: fp.value, quantity: Quantity::Power },
        _ => Value::Measurement { value: fp.value, quantity: Quantity::Generic },
    }
}

const fn temperature_value(fp: &FixedPoint) -> Value {
    let unit = if fp.scale == 1 {
        TemperatureUnit::Fahrenheit
    } else {
        TemperatureUnit::Celsius
    };
    Value::Temperature { value: fp.value, unit }
}

fn push_fixed(out: &mut Vec<u8>, fp: &FixedPoint) {
    // Re-encode using the precision/scale/size the value already carries; this
    // is infallible because a FixedPoint only ever holds legal field values.
    match sensor_decode::encode(fp.value, fp.precision, fp.scale, fp.size) {
        Ok(bytes) => out.extend_from_slice(&bytes),
        // A FixedPoint produced by decode() always re-encodes; on the
        // theoretical overflow path fall back to a zero value of size 1 rather
        // than panicking.
        Err(_) => out.extend_from_slice(&[0x00, 0x00]),
    }
}

fn push_signed(out: &mut Vec<u8>, value: i32, size: u8) {
    match size {
        2 => out.extend_from_slice(&(value as i16).to_be_bytes()),
        4 => out.extend_from_slice(&value.to_be_bytes()),
        // size 1 and any out-of-spec width fall back to a single signed octet.
        _ => out.push(value as i8 as u8),
    }
}

fn read_signed(bytes: &[u8], size: u8) -> ZwaveResult<i32> {
    if !matches!(size, 1 | 2 | 4) {
        return Err(ZwaveError::BadValueSize { size });
    }
    let size_us = size as usize;
    if bytes.len() < size_us {
        return Err(ZwaveError::Truncated { need: size_us + 4, got: bytes.len() + 4 });
    }
    Ok(match size {
        1 => i32::from(bytes[0] as i8),
        2 => i32::from(i16::from_be_bytes([bytes[0], bytes[1]])),
        _ => i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(cmd: Command) {
        let bytes = cmd.encode();
        let back = Command::decode(&bytes).expect("re-decodes");
        assert_eq!(back, cmd, "round-trip mismatch for {cmd:?} via {bytes:02x?}");
    }

    #[test]
    fn command_class_id_roundtrip() {
        for cc in [
            CommandClass::Basic,
            CommandClass::BinarySwitch,
            CommandClass::MultilevelSwitch,
            CommandClass::BinarySensor,
            CommandClass::MultilevelSensor,
            CommandClass::Meter,
            CommandClass::ColorSwitch,
            CommandClass::ThermostatSetpoint,
            CommandClass::Configuration,
            CommandClass::Notification,
            CommandClass::Battery,
        ] {
            assert_eq!(CommandClass::from_u8(cc.to_u8()), Some(cc));
        }
        assert_eq!(CommandClass::from_u8(0xEE), None);
    }

    #[test]
    fn basic_roundtrips() {
        roundtrip(Command::BasicSet(0));
        roundtrip(Command::BasicSet(99));
        roundtrip(Command::BasicSet(0xFF));
        roundtrip(Command::BasicGet);
        roundtrip(Command::BasicReport(50));
    }

    #[test]
    fn basic_set_known_bytes() {
        assert_eq!(Command::BasicSet(0xFF).encode(), vec![0x20, 0x01, 0xFF]);
        assert_eq!(
            Command::decode(&[0x20, 0x03, 0x00]).expect("ok"),
            Command::BasicReport(0)
        );
    }

    #[test]
    fn binary_switch_roundtrips_and_sentinels() {
        roundtrip(Command::BinarySwitchSet(true));
        roundtrip(Command::BinarySwitchSet(false));
        roundtrip(Command::BinarySwitchGet);
        roundtrip(Command::BinarySwitchReport(true));
        // On is 0xFF, off is 0x00 per spec.
        assert_eq!(Command::BinarySwitchSet(true).encode(), vec![0x25, 0x01, 0xFF]);
        assert_eq!(Command::BinarySwitchSet(false).encode(), vec![0x25, 0x01, 0x00]);
        // Any non-zero decodes as "on".
        assert_eq!(
            Command::decode(&[0x25, 0x03, 0x01]).expect("ok"),
            Command::BinarySwitchReport(true)
        );
    }

    #[test]
    fn multilevel_switch_roundtrips() {
        roundtrip(Command::MultilevelSwitchSet(0));
        roundtrip(Command::MultilevelSwitchSet(99));
        roundtrip(Command::MultilevelSwitchSet(0xFF));
        roundtrip(Command::MultilevelSwitchGet);
        roundtrip(Command::MultilevelSwitchReport(42));
        roundtrip(Command::MultilevelSwitchStartLevelChange(LevelChange::Up));
        roundtrip(Command::MultilevelSwitchStartLevelChange(LevelChange::Down));
        roundtrip(Command::MultilevelSwitchStopLevelChange);
    }

    #[test]
    fn multilevel_switch_rejects_reserved_level() {
        // 100..=254 are reserved.
        assert!(matches!(
            Command::decode(&[0x26, 0x03, 0x64]),
            Err(ZwaveError::OutOfRange { .. })
        ));
    }

    #[test]
    fn start_level_change_direction_bit() {
        // Down sets bit 6 (0x40).
        let down = Command::MultilevelSwitchStartLevelChange(LevelChange::Down).encode();
        assert_eq!(down[2] & 0x40, 0x40);
        let up = Command::MultilevelSwitchStartLevelChange(LevelChange::Up).encode();
        assert_eq!(up[2] & 0x40, 0x00);
    }

    #[test]
    fn binary_sensor_roundtrips() {
        roundtrip(Command::BinarySensorGet);
        roundtrip(Command::BinarySensorReport(true));
        roundtrip(Command::BinarySensorReport(false));
    }

    #[test]
    fn multilevel_sensor_temperature_report() {
        // sensor type 0x01 (air temp), precision 1, scale 0, size 2, 24.4 °C.
        let bytes = [0x31, 0x03, 0x01, 0x22, 0x00, 0xF4];
        let cmd = Command::decode(&bytes).expect("valid sensor report");
        match cmd {
            Command::MultilevelSensorReport { sensor_type, reading } => {
                assert_eq!(sensor_type, 0x01);
                assert!((reading.value - 24.4).abs() < 1e-9);
            }
            other => unreachable_variant(&format!("{other:?}")),
        }
        // value model: a temperature in Celsius.
        assert_eq!(
            cmd.to_value(),
            Some(Value::Temperature { value: 24.4, unit: TemperatureUnit::Celsius })
        );
        roundtrip(cmd);
    }

    #[test]
    fn multilevel_sensor_humidity_value_model() {
        // sensor type 0x05 (humidity), precision 0, size 1, 42 %.
        let cmd = Command::decode(&[0x31, 0x03, 0x05, 0x01, 0x2A]).expect("ok");
        assert_eq!(cmd.to_value(), Some(Value::Humidity(42.0)));
    }

    #[test]
    fn multilevel_sensor_get_roundtrips() {
        roundtrip(Command::MultilevelSensorGet);
    }

    #[test]
    fn meter_report_roundtrips_and_value() {
        // meter type 1 (electric), precision 3, scale 0 (kWh), size 4.
        let bytes = [0x32, 0x03, 0x01, 0x64, 0x00, 0x00, 0x27, 0x10];
        let cmd = Command::decode(&bytes).expect("valid meter report");
        match cmd {
            Command::MeterReport { meter_type, reading } => {
                assert_eq!(meter_type, 0x01);
                // 0x2710 = 10000, precision 3 => 10.0 kWh.
                assert!((reading.value - 10.0).abs() < 1e-9);
            }
            other => unreachable_variant(&format!("{other:?}")),
        }
        roundtrip(cmd);
    }

    #[test]
    fn color_switch_roundtrips() {
        roundtrip(Command::ColorSwitchSet { component: 2, value: 200 });
        roundtrip(Command::ColorSwitchGet { component: 2 });
        roundtrip(Command::ColorSwitchReport { component: 4, value: 0 });
        assert_eq!(
            Command::ColorSwitchSet { component: 2, value: 200 }.to_value(),
            Some(Value::ColorComponent { component: 2, intensity: 200 })
        );
    }

    #[test]
    fn thermostat_setpoint_roundtrips() {
        let bytes = [0x43, 0x03, 0x01, 0x22, 0x00, 0xD2];
        let cmd = Command::decode(&bytes).expect("valid setpoint report");
        match cmd {
            Command::ThermostatSetpointReport { setpoint_type, value } => {
                assert_eq!(setpoint_type, 0x01);
                // 0x00D2 = 210, precision 1 => 21.0 °C.
                assert!((value.value - 21.0).abs() < 1e-9);
            }
            other => unreachable_variant(&format!("{other:?}")),
        }
        roundtrip(cmd);
        roundtrip(Command::ThermostatSetpointGet { setpoint_type: 1 });
    }

    #[test]
    fn configuration_roundtrips_all_sizes() {
        roundtrip(Command::ConfigurationSet { parameter: 3, size: 1, value: -5 });
        roundtrip(Command::ConfigurationSet { parameter: 7, size: 2, value: 1000 });
        roundtrip(Command::ConfigurationSet { parameter: 9, size: 4, value: -100_000 });
        roundtrip(Command::ConfigurationGet { parameter: 3 });
        roundtrip(Command::ConfigurationReport { parameter: 3, size: 2, value: -1 });
    }

    #[test]
    fn configuration_signed_byte_decode() {
        // parameter 3, size 1, value 0xFB = -5.
        let cmd = Command::decode(&[0x70, 0x03, 0x03, 0x01, 0xFB]).expect("ok");
        assert_eq!(cmd, Command::ConfigurationReport { parameter: 3, size: 1, value: -5 });
    }

    #[test]
    fn notification_report_roundtrips() {
        // smoke alarm (type 0x01), event "smoke detected" 0x02.
        let cmd = Command::NotificationReport { notification_type: 0x01, event: 0x02 };
        roundtrip(cmd);
        assert_eq!(
            cmd.to_value(),
            Some(Value::Notification { notification_type: 0x01, event: 0x02 })
        );
    }

    #[test]
    fn notification_report_decode_known_bytes() {
        let cmd = Command::decode(&[0x71, 0x05, 0x00, 0x00, 0x00, 0x07, 0xFF, 0x08])
            .expect("valid notification");
        assert_eq!(
            cmd,
            Command::NotificationReport { notification_type: 0x07, event: 0x08 }
        );
    }

    #[test]
    fn battery_roundtrips_and_low_sentinel() {
        roundtrip(Command::BatteryGet);
        roundtrip(Command::BatteryReport(100));
        roundtrip(Command::BatteryReport(0));
        roundtrip(Command::BatteryReport(0xFF));
        // 0xFF maps to the BatteryLow value, not a percentage.
        assert_eq!(Command::BatteryReport(0xFF).to_value(), Some(Value::BatteryLow));
        assert_eq!(Command::BatteryReport(80).to_value(), Some(Value::BatteryPercent(80)));
    }

    #[test]
    fn battery_rejects_impossible_percentage() {
        // 101..=254 are neither a valid percentage nor the low sentinel.
        assert!(matches!(
            Command::decode(&[0x80, 0x03, 0x65]),
            Err(ZwaveError::OutOfRange { .. })
        ));
    }

    #[test]
    fn truncated_payloads_are_rejected_not_panicked() {
        // Empty.
        assert!(matches!(Command::decode(&[]), Err(ZwaveError::Truncated { .. })));
        // CC byte only.
        assert!(matches!(Command::decode(&[0x20]), Err(ZwaveError::Truncated { .. })));
        // Basic Set with no value byte.
        assert!(matches!(Command::decode(&[0x20, 0x01]), Err(ZwaveError::Truncated { .. })));
        // Sensor report claiming size 4 with no value bytes.
        assert!(matches!(
            Command::decode(&[0x31, 0x03, 0x01, 0x0C]),
            Err(ZwaveError::Truncated { .. })
        ));
        // Color set missing the value byte.
        assert!(matches!(
            Command::decode(&[0x33, 0x01, 0x02]),
            Err(ZwaveError::Truncated { .. })
        ));
    }

    #[test]
    fn unknown_command_class_and_command() {
        assert!(matches!(
            Command::decode(&[0xEE, 0x01, 0x00]),
            Err(ZwaveError::UnknownCommand { .. })
        ));
        // Basic CC with an unmodelled command id 0x09.
        assert!(matches!(
            Command::decode(&[0x20, 0x09, 0x00]),
            Err(ZwaveError::UnknownCommand { .. })
        ));
    }

    #[test]
    fn get_commands_have_no_value() {
        assert_eq!(Command::BasicGet.to_value(), None);
        assert_eq!(Command::BatteryGet.to_value(), None);
        assert_eq!(Command::MultilevelSensorGet.to_value(), None);
    }

    // Test-only helper so the assertion failure path is explicit without a bare
    // panic! call sprinkled through the suite. The condition is genuinely
    // data-dependent (it is only ever called from the unexpected-variant arm).
    fn unreachable_variant(what: &str) {
        assert!(what.is_empty(), "unexpected command variant: {what}");
    }
}
