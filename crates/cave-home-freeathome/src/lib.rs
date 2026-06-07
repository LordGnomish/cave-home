// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-freeathome` — Busch-Jaeger free@home System Access Point (SysAP)
//! Local API client (ADR-011, Phase 1b transport layer).
//!
//! [`cave_home_free_home`] is the **brain**: it models the free@home topology,
//! decodes datapoint values and projects channels onto grandma-friendly device
//! kinds — but it is pure logic and deliberately speaks no network. This crate
//! is the **nervous system**: it talks to a real SysAP over its documented
//! local HTTPS API and feeds the brain.
//!
//! ```text
//!   SysAP  ──HTTPS REST───▶  rest + model   ─┐
//!     │                                       ├─▶  state cache  ─▶  core / mqtt bridge
//!     └────WSS push────────▶  event parser  ──┘
//! ```
//!
//! # Modules
//! - [`error`] — the crate error type and `Result` alias.
//! - [`auth`] — HTTP Basic credentials (and the seam for later client-cert / mTLS).
//! - [`config`] — connection configuration and SysAP URL derivation.
//!
//! All domain types (ids, datapoint value codec, pairings, device kinds) are
//! re-used from [`cave_home_free_home`] — this crate never re-implements them.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
// This crate's docs are dense with protocol acronyms (SysAP, HTTPS, WSS, mTLS,
// fhapi); backticking every one hurts readability more than it helps.
#![allow(clippy::doc_markdown)]

pub mod auth;
pub mod config;
pub mod error;
pub mod event;
pub mod model;
pub mod reconnect;
pub mod rest;
pub mod state;

pub use auth::{AuthMethod, ClientCertConfig, Credentials};
pub use config::ClientConfig;
pub use error::{FreeAtHomeError, Result};
pub use event::{parse_datapoint_address, parse_ws_frame, DatapointUpdate, FreeAtHomeEvent};
pub use reconnect::Backoff;
pub use state::StateCache;
pub use model::{
    ChannelDto, ConfigurationResponse, DatapointDto, DeviceDto, DeviceListResponse, SysApConfig,
};
pub use rest::{HttpMethod, RestRequest};
