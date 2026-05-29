//! `cave-home-doorbell` — the front-door doorbell / intercom brain for
//! cave-home (ADR-018).
//!
//! This crate is the **call engine** behind a household doorbell: it models the
//! states a single front-door interaction moves through, runs the ring-timeout
//! state machine, decides whether to chime indoors (do-not-disturb, quiet hours
//! that may wrap midnight, per-event toggles), de-duplicates a burst of repeated
//! rings/motion into one visit, asks the camera pillar for the right kind of
//! media, keeps a visitor log, and turns all of it into a grandma-friendly line
//! in EN / DE / TR.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`event`] — the [`CallState`] set and the [`DoorbellEvent`] vocabulary.
//! - [`machine`] — the [`CallMachine`] state machine: press/motion → ringing →
//!   answered / declined / missed, answered → ended, illegal-transition
//!   rejection, and the caller-supplied ring timeout. The crate reads no clock.
//! - [`chime`] — the [`ChimePolicy`]: tone selection plus do-not-disturb,
//!   quiet-hours (midnight-wrapping) and per-event chime gating.
//! - [`cooldown`] — pure motion/ring de-duplication over a caller-supplied last
//!   accepted tick.
//! - [`media`] — the [`MediaRequest`] model the camera pillar fulfils (snapshot
//!   vs short clip + reason). Capture itself is deferred.
//! - [`log`] — the bounded, append-only [`VisitorLog`] for history.
//! - [`label`] — grandma-friendly EN/DE/TR notification lines (Charter §6.3).
//!
//! The **doorbell hardware adapters** (Reolink / DoorBird / Amcrest / UniFi /
//! Ring-RTSP / Aqara over their own protocols), the **two-way SIP/WebRTC
//! audio**, the **camera-pillar snapshot/clip capture** itself, and the
//! **cave-home-core event-bus integration** are network/hardware-bound and
//! deferred to Phase 1b — every one is enumerated in `parity.manifest.toml`
//! `[[unmapped]]` with an ADR-018 disposition. They drive (or are driven by)
//! this engine without changing it. Ring stays RTSP-only: there is **no** Ring
//! cloud-account integration (Charter §9, ADR-018).
//!
//! # Example
//!
//! ```
//! use cave_home_doorbell::{
//!     CallMachine, CallState, ChimePolicy, ChimeTone, DoorbellEvent, Hour,
//!     Lang, MediaKind, MediaRequest, label,
//! };
//!
//! // A doorbell whose unanswered rings time out after 30 seconds.
//! let mut door = CallMachine::new(30);
//!
//! // The button is pressed at t=0. The bell rings; we ask the camera pillar
//! // for a snapshot of whoever is there.
//! assert_eq!(door.apply(DoorbellEvent::ButtonPressed, 0).unwrap(), CallState::Ringing);
//! let req = MediaRequest::for_event(DoorbellEvent::ButtonPressed, 0).unwrap();
//! assert_eq!(req.kind, MediaKind::Snapshot);
//!
//! // Indoors, the default policy chimes a friendly "ding-dong".
//! let chime = ChimePolicy::default().decide(DoorbellEvent::ButtonPressed, Hour::new(14).unwrap());
//! assert_eq!(chime.tone, ChimeTone::DingDong);
//!
//! // Nobody picks up. 30 seconds later the visit is recorded as missed.
//! assert_eq!(door.tick(30), CallState::Missed);
//! assert_eq!(label::for_state(CallState::Missed, Lang::En), "You missed a visitor");
//! ```

pub mod chime;
pub mod cooldown;
pub mod event;
pub mod label;
pub mod log;
pub mod machine;
pub mod media;

pub use chime::{ChimeDecision, ChimePolicy, ChimeReason, ChimeTone, Hour, HourError};
pub use cooldown::{dedup, CooldownGate, Dedup};
pub use event::{CallState, DoorbellEvent, Tick};
pub use label::{Lang, TimeError, TimeOfDay};
pub use log::{VisitorEntry, VisitorLog};
pub use machine::{CallError, CallMachine};
pub use media::{MediaKind, MediaReason, MediaRequest};
