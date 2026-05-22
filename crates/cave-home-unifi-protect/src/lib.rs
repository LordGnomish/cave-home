// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_truncation)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
//! cave-home-unifi-protect — UniFi Protect port.
//!
//! Line-by-line port of `homeassistant/components/unifiprotect/` from
//! home-assistant/core tag `2026.5.2`
//! (SHA `456202325ac48549bd3c895dc3e69ecd3e2ba6a4`).
//!
//! Phase 1 surface (per ADR-009):
//! - [`nvr`]           — `NvrConfig` + `ProtectClient` + `ProtectNvr`.
//! - [`camera`]        — `ProtectCamera` + `CameraChannel` + ADR-007
//!   `friendly_camera_label`.
//! - [`event`]         — `EventKind` (motion, ring, smart-detect zone /
//!   line, fingerprint, NFC, vehicle) + `ProtectEvent` data.
//! - [`identifiers`]   — `CameraId`, `NvrId`, `EventId` newtypes.
//! - [`error`]         — `ProtectError` covering auth, connect, timeout,
//!   version-too-old, WS-lost.
//! - [`frigate_seam`]  — Protect ↔ Frigate ownership table. See
//!   `docs/upstream/unifi-protect-frigate-handoff.md`.
//! - [`const_table`]   — verbatim HA const port (`DOMAIN`,
//!   `MIN_REQUIRED_PROTECT_V`, `EVENT_TYPE_*`, ...).
//!
//! Convergence with `cave-home-camera` (Frigate): the Portal renders
//! both `ProtectCamera` and `cave_home_camera::CameraConfig` instances
//! through a single grid; `FrigateSeam` is the per-camera decision
//! table that picks which subsystem owns the event stream.
//!
//! Phase 2 backlog:
//! - Wire-side REST bootstrap + WebSocket subscription against UniFi
//!   Protect NVR v6.
//! - PTZ + privacy-zone surfaces (HA `select.py`, `number.py`).
//! - Lock / siren / chime / doorlock entities (HA `lock.py`,
//!   `siren.py`).
//! - Media-source browse path (HA `media_source.py`).
//! - Full `EventType` enum parity (~25 variants total).

pub mod camera;
pub mod const_table;
pub mod error;
pub mod event;
pub mod frigate_seam;
pub mod identifiers;
pub mod nvr;

pub use camera::{CameraChannel, ProtectCamera, friendly_camera_label};
pub use const_table::MIN_PROTECT_VERSION;
pub use error::{ProtectError, ProtectResult};
pub use event::{EventKind, ProtectEvent};
pub use frigate_seam::{FrigateSeam, ProtectSubsystem};
pub use identifiers::{CameraId, EventId, NvrId};
pub use nvr::{NvrConfig, ProtectClient, ProtectNvr};
