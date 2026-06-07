// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Tesla **Fleet API** cloud control plane.
//!
//! - [`auth`] — OAuth2 Authorization-Code + PKCE (S256) flow.
//! - [`client`] — the rate-limited, transport-injected request model.
//! - [`endpoints`] — the `/api/1/energy_sites/*` request builders.
//! - [`types`] — the wire DTOs returned by those endpoints.

pub mod auth;
