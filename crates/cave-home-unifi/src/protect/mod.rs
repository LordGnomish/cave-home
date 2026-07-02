// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi **Protect** REST + update-WebSocket surface.
//!
//! - [`types`] — the bootstrap / camera / event wire DTOs mapped onto
//!   [`cave_home_unifi_protect`], the RTSPS live-URL builder, and the binary
//!   update-packet header ([`ProtectPacketHeader`]).
//! - [`api`] — [`ProtectApi`]: bootstrap, cameras, the live RTSPS URL, the
//!   event log and per-camera recordings, over the console session.

pub mod api;
pub mod types;

pub use api::{Bootstrap, ProtectApi};
pub use types::{ProtectPacketHeader, WireBootstrap, WireCamera, WireEvent, RTSPS_PORT};
