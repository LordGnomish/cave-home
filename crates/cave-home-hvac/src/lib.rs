//! `cave-home-hvac` — the climate / thermostat decision engine for cave-home
//! (ADR-012).
//!
//! This crate is the **brain** that turns a household's comfort wishes into a
//! concrete instruction for a heating, cooling or air-conditioning device. It
//! models the Home Assistant `climate` entity-domain vocabulary
//! ([`HvacMode`], [`HvacAction`], fan and preset modes), validates setpoints and
//! device capabilities, and runs a hysteresis control decision — the same
//! dead-band logic a real thermostat uses to avoid switching on and off every
//! few seconds. Every user-facing string is grandma-friendly and localised in
//! EN / DE / TR (Charter §6.3, ADR-007).
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`temperature`] — the canonical-Celsius temperature value object, with
//!   validated range and C↔F conversion.
//! - [`mode`] — the climate modes, the current activity, fan modes and presets.
//! - [`setpoint`] — single-target and low/high band setpoints (with the
//!   `low < high` invariant) plus device [`Capabilities`] and capability gating.
//! - [`control`] — the hysteresis decision engine ([`decide`]), the surface the
//!   rest of cave-home consumes.
//! - [`label`] — grandma-friendly EN / DE / TR labels and advice.
//!
//! The **vendor I/O adapters** (Viessmann Open3EClient, Daikin, LG ThinQ,
//! Bosch, Mitsubishi, Samsung; generic Zigbee / Z-Wave / Matter thermostats),
//! scheduling / PID tuning, and the cave-home-core entity/state integration are
//! network/hardware-bound and are deferred to Phase 1b — every one is
//! enumerated in `parity.manifest.toml` `[[unmapped]]` with an ADR-012
//! disposition. They map their wire formats onto these types and then reuse this
//! engine unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_hvac::{decide, HvacAction, HvacMode, Setpoint, Temperature, Tolerance};
//!
//! // The room is at 20 °C, the household wants 21 °C with a half-degree
//! // dead-band. The engine says: start heating.
//! let room = Temperature::from_celsius(20.0).unwrap();
//! let target = Setpoint::single(Temperature::from_celsius(21.0).unwrap());
//! let tol = Tolerance::symmetric(0.5).unwrap();
//!
//! let action = decide(HvacMode::Heat, room, &target, tol).unwrap();
//! assert_eq!(action, HvacAction::Heating);
//! ```

pub mod control;
pub mod label;
pub mod mode;
pub mod setpoint;
pub mod temperature;

pub use control::{decide, DecideError, Tolerance, ToleranceError};
pub use label::Lang;
pub use mode::{FanMode, HvacAction, HvacMode, PresetMode};
pub use setpoint::{Capabilities, CapabilityError, Setpoint, SetpointError};
pub use temperature::{
    celsius_to_fahrenheit, fahrenheit_to_celsius, Temperature, TemperatureError, MAX_CELSIUS,
    MIN_CELSIUS,
};
