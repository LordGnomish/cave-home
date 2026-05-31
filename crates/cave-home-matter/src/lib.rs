// SPDX-License-Identifier: Apache-2.0
//! cave-home-matter — Matter 1.3+ stack.
//!
//! Line-by-line port of `src/` from `project-chip/connectedhomeip`
//! v1.3.0.0 (SHA `5af45c5cd17ee8df03b8c8e7f6b1f7d3f6e1a4d8`). The
//! upstream license is Apache-2.0 → permissive, so line-by-line is
//! the agreed port method per ADR-002.
//!
//! Phase 1 MVP scope (commissioner role):
//! - [`setup_payload`]  — QR code + manual pairing code parser.
//! - [`pase`]           — PASE (Spake2+) session establishment.
//! - [`case`]           — CASE operational session.
//! - [`fabric`]         — FabricTable (households).
//! - [`acl`]            — AccessControl + Privilege.
//! - [`group_key`]      — GroupDataProvider (group keys).
//! - [`ota`]            — OTA-Requestor cluster client.
//! - [`clusters`]       — OnOff, LevelControl, ColorControl, Thermostat,
//!                        DoorLock, WindowCovering, NetworkCommissioning clients.
//! - [`transport`]      — UDP + BLE GATT commissioning transports.
//! - [`commissioner`]   — DeviceCommissioner (pair / unpair).
//! - [`controller`]     — top-level Controller composition.
//! - [`error`]          — `MatterError`.
//! - [`prelude`]        — re-exports for downstream crates.
//!
//! Out of Phase 1 scope: device-side (accessory) Server, full
//! Interaction Model engine, mDNS browse, DAC chain validation,
//! BDX, ICD — see `parity.manifest.toml` `[[unmapped]]`.
//!
//! # UX vocabulary (Charter §6.3, ADR-007)
//! The user-visible Portal + cavectl surface labels a Matter fabric
//! as **"Hane"** / **"Ev"** (household), never "fabric"; nodes are
//! **"Cihaz"** (device); pairing is **"Eşle"**. The internal types
//! keep the upstream chip vocabulary for line-by-line parity, the
//! translation lives at the UI layer (`cave-home-portal::admin::matter`).

pub mod acl;
pub mod case;
pub mod clusters;
pub mod commissioner;
pub mod controller;
pub mod error;
pub mod fabric;
pub mod group_key;
pub mod ota;
pub mod pase;
pub mod prelude;
pub mod setup_payload;
pub mod transport;

pub use error::MatterError;
