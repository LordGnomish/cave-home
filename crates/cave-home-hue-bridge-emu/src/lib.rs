// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! cave-home-hue-bridge-emu — Hue Bridge emulator.
//!
//! **Clean-room** Rust implementation written from the Philips developer-
//! portal Hue API v1 + v2 public documentation (ADR-010 + Charter §6.1).
//! The conceptual reference (`diyhue/diyHue`) is **GPL-3.0**; its source is
//! NEVER consulted. Every file in this crate carries the clean-room banner.
//!
//! ## Scope — Phase 1 MVP
//!
//! - [`config`]    — emulated bridge identity (UUID, MAC, bridge ID, model).
//! - [`registry`]  — in-memory registry of lights, groups, scenes, sensors
//!                   plus an event-stream broadcaster.
//! - [`pairing`]   — `POST /api` pairing flow with link-button window.
//! - [`api::v1`]   — `/api/{appkey}/...` v1 REST endpoints.
//! - [`api::v2`]   — `/clip/v2/resource/...` v2 endpoints + `/clip/v2/eventstream`.
//! - [`discovery`] — `/description.xml` (UPnP SSDP) + `/api/config`
//!                   anonymous probe + mDNS `_hue._tcp` advertisement-payload
//!                   builder.
//!
//! ## Charter v6 / ADR-007 — advanced-mode toggle
//!
//! This crate is **advanced-mode only**. The Portal exposes the
//! "cave-home'u Hue Bridge olarak yayınla" toggle exclusively behind
//! Settings → Developer view. The headline persona never sees the option.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod api;
pub mod config;
pub mod discovery;
pub mod errors;
pub mod pairing;
pub mod registry;
