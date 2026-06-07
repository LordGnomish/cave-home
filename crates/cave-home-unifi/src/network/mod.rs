// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi **Network Controller** REST surface.
//!
//! - [`types`] — the `{meta, data}` envelope, the `stat/*` + `self/sites` wire
//!   DTOs, and their mapping onto the [`cave_home_unifi_network`] domain model.
//! - [`api`] — [`NetworkApi`]: sites / clients / devices / events / health
//!   reads and block / unblock / reconnect / PoE writes, plus
//!   [`NetworkApi::execute`] for a domain control [`Command`](cave_home_unifi_network::Command).

pub mod api;
pub mod types;

pub use api::NetworkApi;
pub use types::{HealthSubsystem, NetworkEvent, Site};
