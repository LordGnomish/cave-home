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
//! cave-home-unifi-talk — UniFi Talk VoIP / intercom port.
//!
//! HA core has **no** `unifi_talk` integration in
//! home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4 (tag 2026.5.2).
//! ADR-009 §4 explicitly caps this crate's parity at "whatever Ubiquiti
//! exposes" — Phase 1 surfaces the roster + call lifecycle event types
//! + the four call-control verbs (answer / decline / transfer / end).
//! Wire-side REST calls are gated behind `TalkError::Unavailable` until
//! Ubiquiti stabilises the public endpoint.
//!
//! Phase 1 surface:
//! - [`client`] — `TalkConfig` + `TalkClient` (API-token auth).
//! - [`phone`]  — `TalkPhone`, `PhoneId`, `PhoneRoster`,
//!   ADR-007 `friendly_phone_label`.
//! - [`call`]   — `IncomingCall`, `CallEvent`, `CallEventKind`,
//!   `CallControlVerb`, `CallId`.
//! - [`error`]  — `TalkError`.
//!
//! Phase 2 backlog (Ubiquiti API stability dependent):
//! - Live call control over REST (`control_call` actually issuing
//!   `POST /api/talk/calls/{id}/{verb}`).
//! - Voicemail browse + playback.
//! - Call history paging.
//! - DND / forwarding rules.

pub mod call;
pub mod client;
pub mod error;
pub mod phone;

pub use call::{CallControlVerb, CallEvent, CallEventKind, CallId, IncomingCall};
pub use client::{TalkClient, TalkConfig};
pub use error::{TalkError, TalkResult};
pub use phone::{PhoneId, PhoneRoster, TalkPhone, friendly_phone_label};
