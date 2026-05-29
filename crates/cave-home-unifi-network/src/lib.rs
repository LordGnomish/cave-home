// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::uninlined_format_args)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
//! `cave-home-unifi-network` — the home-network brain for cave-home (ADR-009).
//!
//! This crate is the **decision engine** for a UniFi-style home network: it
//! models the devices (switches, access points, the gateway) and the clients
//! (phones, tablets, laptops) on the network, validates the control operations
//! a household actually performs — *block the kid's tablet, kick a stuck
//! laptop off Wi-Fi, turn the guest network on, set a switch port to power a
//! camera* — and turns raw network state into a plain-language summary a
//! grandmother can read: "12 things connected, Guest Wi-Fi is on, internet is
//! up".
//!
//! # Scope (Phase 1 MVP — pure logic, std-only)
//!
//! Implemented, real and tested here:
//! - [`device`]   — [`NetworkDevice`] model (switch / AP / gateway, online
//!   state, uplink, switch ports with `PoE`).
//! - [`client`]   — [`NetworkClient`] model (wired / wireless, IP, SSID,
//!   guest / blocked flags, last-seen tick).
//! - [`control`]  — validated control operations (block / unblock, reconnect,
//!   `PoE` port mode, `WLAN` enable / disable, port-forward toggle, device LED).
//!   Every operation validates its inputs and yields a typed [`Command`].
//! - [`presence`] — device-tracker home / away derivation from last-seen + a
//!   timeout, the input to presence automations.
//! - [`network`]  — [`Wlan`], [`PortForward`], [`GuestNetwork`],
//!   [`BandwidthProfile`] and the [`Vlan`] / subnet model.
//! - [`summary`]  — connectivity summary: per-AP client counts, throughput
//!   aggregation over samples, and the "is the internet up" derivation.
//! - [`label`]    — the grandma-friendly EN / DE / TR phrasing (Charter §6.3,
//!   ADR-007).
//!
//! # Deferred to Phase 1b (see `parity.manifest.toml` `[[unmapped]]`)
//!
//! The `UniFi` controller `REST` login, the `WebSocket` event transport, the actual
//! API calls and controller-version negotiation are all **network-bound** and
//! deferred per ADR-009. They feed their wire formats onto the models in this
//! crate and reuse this engine unchanged. cave-home stays cloud-free: only the
//! **local** controller API is ever in scope (Charter §9 — no Ubiquiti cloud).
//!
//! # Example
//!
//! ```
//! use cave_home_unifi_network::{NetworkClient, control, Command, Lang, label};
//!
//! // The kid's tablet is on Wi-Fi; a bedtime automation blocks it.
//! let tablet = NetworkClient::new("aa:bb:cc:dd:ee:01", "Kid's tablet")
//!     .wireless("Home", "ap-1");
//! let cmd = control::block_client(&tablet).unwrap();
//! assert_eq!(cmd, Command::BlockClient { mac: "aa:bb:cc:dd:ee:01".to_string() });
//!
//! // The household sees plain language, never a MAC address.
//! assert_eq!(label::client_blocked("Kid's tablet", Lang::En), "Kid's tablet is blocked");
//! ```

pub mod client;
pub mod control;
pub mod device;
pub mod label;
pub mod network;
pub mod presence;
pub mod summary;

pub use client::{ConnectionKind, NetworkClient};
pub use control::{Command, ControlError, PoeMode};
pub use device::{DeviceKind, DeviceState, NetworkDevice, SwitchPort};
pub use label::Lang;
pub use network::{
    BandwidthProfile, GuestNetwork, NetworkPurpose, PortForward, Protocol, Vlan, Wlan,
};
pub use presence::{Presence, presence_of};
pub use summary::{ConnectivitySummary, InternetState, ThroughputSample, summarize};
