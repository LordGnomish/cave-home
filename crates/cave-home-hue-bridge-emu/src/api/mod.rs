// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! HTTP API surface — v1 REST + v2 CLIP.
//!
//! The actual HTTP server is wired by the cave-home binary; this module
//! exposes pure functions that translate `(method, path, body)` triples
//! into JSON responses against [`crate::registry::BridgeRegistry`].

pub mod v1;
pub mod v2;
