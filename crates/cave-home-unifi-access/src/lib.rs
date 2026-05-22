// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_precision_loss)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
//! cave-home-unifi-access — UniFi Access port.
//!
//! Line-by-line port of `homeassistant/components/unifi_access/` from
//! home-assistant/core tag `2026.5.2`
//! (SHA `456202325ac48549bd3c895dc3e69ecd3e2ba6a4`).
//!
//! Phase 1 surface (per ADR-009):
//! - [`client`]      — `AccessConfig` + `AccessClient` (API-token auth).
//! - [`door`]        — `Door`, `DoorId`, `LockRelayStatus`,
//!   `DoorPositionStatus`, `DoorLockRule`, `DoorLockRuleType`,
//!   `EmergencyStatus`, ADR-007 `friendly_door_label`.
//! - [`events`]      — `DoorEvent`, `DoorEventCategory`,
//!   `DoorEventKind` (doorbell ring, access granted/denied).
//! - [`error`]       — `AccessError` covering auth / connect / NotFound
//!   / WS-lost / invalid-rule-type.
//! - [`const_table`] — verbatim HA const port (`DOMAIN`,
//!   `DEFAULT/MIN/MAX_LOCK_RULE_INTERVAL`, ...).
//!
//! Phase 2 backlog:
//! - Wire-side REST + WS against UniFi Access hub.
//! - Image entity for door thumbnails (HA `image.py`).
//! - Button entity for "unlock now" + "press doorbell" (HA `button.py`).
//! - `set_lock_rule` service (HA `services.py`).

pub mod client;
pub mod const_table;
pub mod door;
pub mod error;
pub mod events;

pub use client::{AccessClient, AccessConfig};
pub use const_table::{
    DEFAULT_LOCK_RULE_INTERVAL, MAX_LOCK_RULE_INTERVAL, MIN_LOCK_RULE_INTERVAL,
};
pub use door::{
    Door, DoorId, DoorLockRule, DoorLockRuleType, DoorPositionStatus, EmergencyStatus,
    LockRelayStatus, friendly_door_label,
};
pub use error::{AccessError, AccessResult};
pub use events::{DoorEvent, DoorEventCategory, DoorEventKind};
