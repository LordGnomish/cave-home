// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! EZSP (EmberZNet Serial Protocol).
//!
//! Per Silicon Labs UG100, EZSP is the protocol between a host
//! microcontroller (here cave-home) and an EmberZNet Network Co-Processor
//! (NCP) such as the Sonoff ZBDongle-E or SMLIGHT SLZB-06.
//!
//! Layering:
//! - [`ash`]      — ASH (Async Serial Host) framer: SLIP-style framing
//!                  with sequence numbers + CRC over the host serial link.
//! - [`frame`]    — EZSP application frame format (sequence + frame
//!                  control + frame ID + parameters).
//! - [`commands`] — concrete EZSP commands cave-home Phase 1 uses
//!                  (version, network init, permit-joining, …).

pub mod ash;
pub mod commands;
pub mod frame;

pub use ash::{AshFramer, AshFrame};
pub use commands::{EzspCommand, EzspNetworkParameters, EzspResponse};
pub use frame::{EzspFrame, EzspFrameControl};
