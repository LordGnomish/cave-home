// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-tesla` — talk to a Tesla Powerwall / energy site from cave-home.
//!
//! This crate is the **decision + protocol core** for Tesla's home-energy
//! products (Powerwall, solar, the whole-home energy site). It speaks two
//! surfaces:
//!
//! - the **Tesla Fleet API** ([`fleet_api`]) — the cloud control plane:
//!   OAuth2-PKCE authentication ([`fleet_api::auth`]), a rate-limited request
//!   model ([`fleet_api::client`]), the `/api/1/energy_sites/*` endpoint
//!   surface ([`fleet_api::endpoints`]) and the wire DTOs ([`fleet_api::types`]).
//! - the **Powerwall local Gateway** ([`gateway_local`]) — the on-LAN HTTPS
//!   surface the gateway exposes (meter aggregates, state-of-energy), used as a
//!   low-latency fallback when the cloud is unreachable.
//!
//! On top of both sits a clean [`EnergyProvider`] trait ([`adapter`]) over the
//! grandma-friendly [`models`] domain types ([`SiteStatus`], [`PowerFlowData`],
//! [`BatteryData`], [`OpMode`]). Observability lives in [`metrics`]; credential
//! handling in [`token_store`]; the node configuration model in [`config`].
//!
//! # Honesty & scope (see `parity.manifest.toml`)
//!
//! Everything that is *logic* is implemented and tested here, with no network
//! or clock baked in: PKCE challenge derivation (a first-party SHA-256 +
//! base64url in [`crypto`]), the per-endpoint rate-limit state machine, the
//! exponential 429 back-off, every request builder, the full wire→domain
//! mapping, the last-known-state cache, the Prometheus exposition and the
//! credential model. The crate takes its clock, its randomness and its HTTP
//! transport from the caller — see the [`fleet_api::client::HttpTransport`]
//! seam and the in-crate [`fleet_api::client::MockTransport`] used by the tests.
//!
//! The *real* `reqwest`/TLS transport (cloud + self-signed gateway) is the only
//! network-bound piece and is deferred to Phase 1b exactly as every other
//! cave-home device adapter defers its transport; it implements
//! [`fleet_api::client::HttpTransport`] and changes none of the logic here.
//!
//! # Single-binary
//!
//! Per Charter §5 this is a library crate compiled into the one cave-home
//! binary as part of the `Integrations` pillar — never a separate pod or Helm
//! release.

#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]

// Modules land test-first, one TDD cycle at a time (see git history and
// `parity.manifest.toml`).

pub mod crypto;
