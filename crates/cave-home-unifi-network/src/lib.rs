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
//! cave-home-unifi-network — UniFi Network port.
//!
//! Line-by-line port of `homeassistant/components/unifi/` from
//! home-assistant/core tag `2026.5.2`
//! (SHA `456202325ac48549bd3c895dc3e69ecd3e2ba6a4`).
//!
//! Phase 1 surface (per ADR-009):
//! - [`controller`] — `ControllerConfig` + `UnifiController` connection
//!   handle.
//! - [`device`]     — `UnifiDevice`, `DeviceKind`, `DeviceState`,
//!   `PortStat`, ADR-007 friendly-label helper.
//! - [`client`]     — `UnifiClient` + `WirelessClientRegistry` (ports HA
//!   `UnifiWirelessClients`).
//! - [`switch`]     — `BlockSwitch`, `OutletSwitch` (ports HA
//!   `UnifiBlockClientSwitch`, `UnifiOutletSwitch`; `DPISwitch` deferred).
//! - [`events`]     — typed `ControllerEvent` enum for the WebSocket
//!   subscription.
//! - [`identifiers`] — `ClientId`, `DeviceId`, `SiteId` newtypes.
//! - [`error`]       — `UnifiError` widening HA's 4 exception classes.
//! - [`const_table`] — verbatim HA const port (`DOMAIN`, `DEVICE_STATES`,
//!   `DEFAULT_*`).
//!
//! Phase 2 backlog:
//! - Wire-side REST + WebSocket I/O against UniFi Network v8 controller.
//! - DPI restriction group switch (`UnifiDPIRestrictionGroupSwitch`).
//! - Device-tracker entity surface (HA `device_tracker.py`).
//! - Update entity surface for firmware upgrades (HA `update.py`).
//! - Image entity for AP placement maps (HA `image.py`).
//! - Persistence of `WirelessClientRegistry` to disk via `Store`.

pub mod client;
pub mod const_table;
pub mod controller;
pub mod device;
pub mod error;
pub mod events;
pub mod identifiers;
pub mod switch;

pub use client::{UnifiClient, WirelessClientRegistry};
pub use controller::{ControllerConfig, UnifiController};
pub use device::{DeviceKind, DeviceState, PortStat, UnifiDevice, friendly_device_label};
pub use error::{UnifiError, UnifiResult};
pub use events::ControllerEvent;
pub use identifiers::{ClientId, DeviceId, SiteId};
pub use switch::{BlockSwitch, OutletSwitch};
