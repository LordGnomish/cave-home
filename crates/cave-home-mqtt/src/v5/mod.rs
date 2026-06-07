//! MQTT 5.0 (OASIS) clean-room codec — Phase 2.
//!
//! MQTT 5.0 extends the 3.1.1 wire format (still a fixed header with a
//! variable-byte Remaining Length, §1.5.5) with two cross-cutting
//! additions modelled here:
//!
//!   * **Properties** (§2.2.2) — a typed, length-prefixed metadata block
//!     present in the variable header (and the PUBLISH/CONNECT will
//!     payload) of almost every v5 packet. See [`property`].
//!   * **Reason Codes** (§2.4) — a one-byte status that replaces the
//!     3.1.1 CONNACK/SUBACK return-code bytes and is added to the ack
//!     packets, DISCONNECT and the new AUTH packet. See [`reason`].
//!
//! Everything is reimplemented from the published OASIS MQTT 5.0
//! standard; no Eclipse Mosquitto (EPL-2.0) source is consulted.

pub mod property;
pub mod reason;
pub(crate) mod wire;

pub use property::{decode_properties, encode_properties, Property};
pub use reason::ReasonCode;
pub use wire::Error;
