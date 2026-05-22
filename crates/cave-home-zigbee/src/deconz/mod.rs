// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! deCONZ serial protocol (dresden-elektronik ConBee II).
//!
//! Implements the public deCONZ serial protocol used by the ConBee II
//! USB stick. Framing is SLIP (RFC 1055) with a CRC-16 (CCITT) appended.
//!
//! Phase 1 ships:
//! - [`slip`]    — SLIP framer (RFC 1055-style byte stuffing + END markers).
//! - [`commands`] — minimal command surface (version, read network params,
//!                  permit-joining, APS-DATA-INDICATION callback).

pub mod commands;
pub mod slip;

pub use commands::{DeconzCommand, DeconzResponse};
pub use slip::{SlipFramer, SLIP_END, SLIP_ESC};
