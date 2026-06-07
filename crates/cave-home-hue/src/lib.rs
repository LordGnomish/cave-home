// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! cave-home-hue — Philips Hue Bridge client.
//!
//! Line-by-line port of the Apache-2.0 upstreams:
//!
//! - `home-assistant/core` v2026.5.2, subpath
//!   `homeassistant/components/hue/` — the HA "hue" integration glue
//!   (bridge lifecycle, config flow, errors, migration), and
//! - `home-assistant-libs/aiohue` v4.8.1 — the underlying Hue API client
//!   (v1 REST + v2 CLIP / EventStream).
//!
//! Upstreams are pinned by release tag, not commit SHA: the previously
//! recorded SHAs were unverified (one hash had been copied across unrelated
//! upstreams) and have been removed. Both upstreams are Apache-2.0, so the
//! port is line-by-line per ADR-002.
//! Each `.rs` file carries a `// Source:` banner naming the upstream file +
//! release tag that was ported. Wherever upstream Python idioms map to Rust traits +
//! async patterns the original API shape is preserved (method names, default
//! values, return shapes) so reviewers can do a line-pair diff.
//!
//! ## Scope — Phase 1 MVP (per ADR-010)
//!
//! - [`util`]      — bridge-ID normalisation, MAC parsing.
//! - [`errors`]    — Hue error taxonomy mirroring `aiohue.errors`.
//! - [`discovery`] — NUPNP, mDNS-style host probe, v2-capability check
//!                   (matches `aiohue.discovery`).
//! - [`v1`]        — Hue API v1 REST surface (`aiohue.v1`: lights, groups,
//!                   scenes, sensors, config).
//! - [`v2`]        — Hue API v2 CLIP surface (`aiohue.v2`: HueBridgeV2,
//!                   EventStream / SSE, controllers + models).
//! - [`bridge`]    — high-level HueBridge wrapper that mirrors HA's
//!                   `homeassistant.components.hue.bridge.HueBridge`.
//!
//! Out-of-MVP (tracked in [`parity.manifest.toml`](../parity.manifest.toml)):
//!   Entertainment / DTLS streaming, the HomeKit / Matter bridges, advanced
//!   v1 rule engine, ConfigFlow re-auth subtleties. See ADR-010 §Open
//!   questions.
//!
//! ## Charter v6 grandma-friendly (ADR-007)
//!
//! Nothing in this crate is grandma-facing. The user-visible vocabulary
//! ("Lamba / Oda / Sahne / Sensör / Düğme") lives in the Portal admin
//! module + cavectl. This crate never expects an end-user to see a raw
//! bridge IP, application key, or resource UUID.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod bridge;
pub mod discovery;
pub mod errors;
pub mod util;
pub mod v1;
pub mod v2;
