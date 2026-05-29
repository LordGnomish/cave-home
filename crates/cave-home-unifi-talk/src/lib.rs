// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
//! `cave-home-unifi-talk` — the household intercom / phone call-control brain
//! for cave-home (ADR-009).
//!
//! UniFi Talk is Ubiquiti's VoIP intercom + desk-phone system: a front-door
//! intercom, a wall panel, a desk phone in the study. This crate is the
//! **call-control engine** behind it — the pure-logic core that decides who
//! rings when a call comes in, walks a single call through its lifecycle
//! (ringing → connecting → talking → on hold → ended), honours do-not-disturb
//! and after-hours routing, models transfers and a three-way conference, keeps
//! a call history, and turns all of it into a grandma-friendly line in
//! EN / DE / TR.
//!
//! It reads no clock and touches no network: the caller supplies "now" as a
//! whole-second [`call::Tick`] (for ring timeouts) and a wall-clock
//! [`schedule::Minute`] (for after-hours routing), so the whole engine is
//! deterministic and trivially testable.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`device`] — the [`device::TalkDevice`] roster model (desk phone /
//!   intercom / doorbell, online / offline).
//! - [`extension`] — the [`extension::Extension`] model (number, display name,
//!   assigned device, voicemail) and the [`extension::CallGroup`] ring group
//!   with its [`extension::RingStrategy`] (ring-all / sequential / round-robin).
//! - [`call`] — the [`call::CallMachine`] state machine: the [`call::CallState`]
//!   set, the [`call::CallEvent`] vocabulary, illegal-transition rejection, the
//!   caller-supplied ring-no-answer timeout that resolves to *missed* or
//!   *voicemail*, hold / resume, blind & attended transfer, and a three-way
//!   [`call::Conference`] membership model.
//! - [`routing`] — given an incoming call to an extension or a ring group,
//!   compute who rings and in what order, honouring per-extension
//!   do-not-disturb (with an emergency override) and call-forwarding.
//! - [`schedule`] — a time-of-day routing schedule: in business hours a call
//!   rings the extension; after hours it goes to voicemail or a forward.
//! - [`log`] — the bounded, append-only [`log::CallLog`] of past calls
//!   (from / to, direction, outcome, duration).
//! - [`label`] — grandma-friendly EN/DE/TR call lines (Charter §6.3, ADR-007).
//!
//! The **transport** (SIP signalling + RTP media + codecs), the **UniFi Talk
//! provisioning REST/WS API**, the actual **audio path**, **PSTN / SIP-trunk**
//! integration, and the **cave-home-doorbell / -core glue** are
//! network/audio/signalling-bound and deferred to Phase 1b — every one is
//! enumerated in `parity.manifest.toml` `[[unmapped]]` with an ADR-009
//! disposition. They drive (or are driven by) this engine without changing it.
//! There is **no** cloud-VoIP relay dependency: routing is local-first
//! (Charter §9).
//!
//! # Example
//!
//! ```
//! use cave_home_unifi_talk::{
//!     CallEvent, CallMachine, CallState, Disposition, Lang, label,
//! };
//!
//! // A call whose unanswered ring times out after 30 seconds, falling back to
//! // voicemail rather than just being recorded as missed.
//! let mut call = CallMachine::new(30, Disposition::Voicemail);
//!
//! // The front-door intercom rings at t=0.
//! assert_eq!(call.apply(CallEvent::Incoming, 0).unwrap(), CallState::Ringing);
//! assert_eq!(label::for_state(CallState::Ringing, Lang::En), "Someone is calling");
//!
//! // Nobody picks up. 30 seconds later the ring rolls to voicemail.
//! assert_eq!(call.tick(30), CallState::Voicemail);
//! assert_eq!(label::for_state(CallState::Voicemail, Lang::En), "Caller left a voicemail");
//! ```

pub mod call;
pub mod device;
pub mod extension;
pub mod label;
pub mod log;
pub mod routing;
pub mod schedule;

pub use call::{
    CallError, CallEvent, CallMachine, CallState, Conference, Disposition, Tick, TransferKind,
};
pub use device::{DeviceId, DeviceKind, DeviceState, TalkDevice};
pub use extension::{CallGroup, Extension, ExtensionError, RingStrategy};
pub use label::Lang;
pub use log::{CallDirection, CallLog, CallRecord};
pub use routing::{ForwardTarget, RingPlan, RouteOutcome, route_call};
pub use schedule::{BusinessHours, Minute, MinuteError};
