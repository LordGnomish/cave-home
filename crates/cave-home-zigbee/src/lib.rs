// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! cave-home-zigbee — Zigbee 3.0 stack.
//!
//! Clean-room reimplementation per Charter §6.1 + ADR-002. The upstream
//! `Koenkk/zigbee2mqtt` is GPL-3.0; this crate is implemented strictly
//! from public specifications (Zigbee 3.0, ZCL, IEEE 802.15.4, Silicon
//! Labs EZSP UG100, deCONZ serial protocol) and the `zigbee-herdsman`
//! npm package public README + `.d.ts` type signatures. The upstream
//! source has not been read.
//!
//! Phase 1 MVP scope:
//! - [`transport`]  — abstracted USB UART + TCP socket transport.
//! - [`ezsp`]       — EZSP frame layer + ASH transport for Silicon Labs
//!                    NCPs (Sonoff ZBDongle-E + SMLIGHT SLZB-06).
//! - [`deconz`]     — deCONZ serial protocol + SLIP framer for the
//!                    dresden-elektronik ConBee II.
//! - [`coordinator`] — coordinator init / form-network entry point.
//! - [`network`]    — NWK + APS layers + routing table.
//! - [`zcl`]        — ZCL frame format + Foundation commands.
//! - [`onoff`]      — OnOff cluster (0x0006) commands + state.
//! - [`level_control`] — Level Control cluster (0x0008) dimming.
//! - [`pairing`]    — network steering / InstallCode / Touchlink.
//! - [`attribute_reporting`] — Configure / Read / Report attributes.
//! - [`groups`]     — Groups cluster (0x0004).
//! - [`scenes`]     — Scenes cluster (0x0005).
//! - [`ota`]        — OTA Upgrade cluster (0x0019) signal handler + queue.
//! - [`events`]     — outbound event stream (DeviceJoined / Report / …).
//! - [`error`]      — crate-wide error type.
//! - [`prelude`]    — re-exports for downstream callers.
//!
//! Out-of-scope items are enumerated in `parity.manifest.toml` under
//! `[[unmapped]]`.

#![doc(html_root_url = "https://docs.rs/cave-home-zigbee")]

pub mod attribute_reporting;
pub mod coordinator;
pub mod deconz;
pub mod error;
pub mod events;
pub mod ezsp;
pub mod groups;
pub mod level_control;
pub mod network;
pub mod onoff;
pub mod ota;
pub mod pairing;
pub mod prelude;
pub mod scenes;
pub mod transport;
pub mod zcl;
