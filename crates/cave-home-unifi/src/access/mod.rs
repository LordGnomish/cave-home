// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi **Access** developer REST + notification WebSocket surface.
//!
//! - [`types`] — the `{code,msg,data}` envelope, the door / visitor / access-log
//!   wire DTOs mapped onto [`cave_home_unifi_access`], and the real-time
//!   [`AccessNotification`] (with intercom-call detection).
//! - [`api`] — [`AccessClient`]: doors / visitors / events reads, unlock /
//!   intercom-answer / lock-rule writes, and the notifications WebSocket URL.

pub mod api;
pub mod types;

pub use api::{AccessClient, AccessConfig, LockRule, DEFAULT_ACCESS_PORT};
pub use types::{
    AccessEnvelope, AccessNotification, DoorStatus, NotificationKind, Visitor,
};
