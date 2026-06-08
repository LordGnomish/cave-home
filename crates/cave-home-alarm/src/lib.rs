//! `cave-home-alarm` — the alarm control-panel safety brain for cave-home
//! (ADR-018).
//!
//! This crate is the **safety-critical state machine** behind a household alarm
//! panel: it models the `alarm_control_panel` states a home can be in, runs the
//! arm (exit-delay) and entry-delay countdowns, sounds the alarm when a watched
//! sensor trips and is not disarmed in time, gates every disarm on a valid user
//! code, and turns the result into a grandma-friendly status line + advice in
//! EN / DE / TR.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`state`] — the [`AlarmState`] set and [`AlarmCommand`] verbs (HA
//!   `alarm_control_panel` domain semantics).
//! - [`config`] — validated per-panel timing/policy ([`PanelConfig`]):
//!   exit/entry/siren delays, code-on-arm, instant zones, stay-triggered.
//! - [`code`] — the validated [`UserCode`] value object (never leaks via
//!   `Debug`), an opaque constant-time-comparable digest, and the brute-force
//!   keypad lock-out ([`CodeCredential`]).
//! - [`machine`] — the [`AlarmPanel`] state machine: arm → arming → armed,
//!   sensor trip → pending → triggered → back to armed, disarm-with-code, and
//!   illegal-transition rejection. Time is supplied by the caller in whole
//!   seconds; the crate reads no clock.
//! - [`label`] — the grandma-friendly EN/DE/TR status label + advice per state
//!   (Charter §6.3, ADR-007).
//!
//! The **sensor/zone hardware adapters** (door/window/motion over
//! Zigbee/Z-Wave/MQTT), the **siren actuator output**, real **cryptographic
//! code hashing**, and **cave-home-core** event-bus integration are
//! hardware/crypto/network-bound and deferred to Phase 1b — every one is
//! enumerated in `parity.manifest.toml` `[[unmapped]]` with an ADR-018
//! disposition. They drive (or are driven by) this engine without changing it.
//!
//! cave-home is the single trust boundary for the alarm pillar: there is **no**
//! third-party alarm-monitoring relay (Charter §9, ADR-018).
//!
//! # Example
//!
//! ```
//! use cave_home_alarm::{
//!     AlarmCommand, AlarmPanel, AlarmState, CodeCredential, Lang, PanelConfig,
//!     UserCode, Zone,
//! };
//!
//! // A panel: 30 s to leave, 20 s to disarm on return, 4-minute siren,
//! // a code required to arm and disarm.
//! let cfg = PanelConfig::new(30, 20, 240, true, false, false, false).unwrap();
//! let code = UserCode::parse("1379").unwrap();
//! let mut panel = AlarmPanel::new(cfg, CodeCredential::enroll(&code));
//!
//! // Arm for an empty house. The exit delay starts.
//! panel.apply_with_code(AlarmCommand::ArmAway, &code).unwrap();
//! assert_eq!(panel.state(), AlarmState::Arming);
//!
//! // 30 seconds pass — the watch begins.
//! assert_eq!(panel.tick(30), AlarmState::ArmedAway);
//!
//! // A door opens. The entry delay starts; the household sees a plain prompt.
//! panel.sensor_trip(Zone::Perimeter);
//! assert_eq!(panel.state(), AlarmState::Pending);
//! assert_eq!(panel.state().label(Lang::En), "Welcome home — enter your code");
//!
//! // They enter the right code in time — the alarm is off.
//! panel.apply_with_code(AlarmCommand::Disarm, &code).unwrap();
//! assert_eq!(panel.state(), AlarmState::Disarmed);
//! ```

pub mod code;
pub mod config;
pub mod label;
pub mod machine;
pub mod state;

pub use code::{CodeCredential, CodeError, CodeVerdict, UserCode};
pub use config::{ConfigError, PanelConfig, Seconds};
pub use label::Lang;
pub use machine::{AlarmError, AlarmPanel, Zone};
pub use state::{AlarmCommand, AlarmState};
