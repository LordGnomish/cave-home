// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::doc_markdown)]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::float_cmp
    )
)]
//! `cave-home-unifi` — the **real** Ubiquiti UniFi transport + API surface for
//! cave-home (the Phase-1b wire layer the `unifi-network` / `unifi-access` /
//! `unifi-protect` domain cores deferred under ADR-009).
//!
//! The three sibling crates own the pure decision logic — the network device /
//! client model, the door-access safety brain, the Protect detection brain —
//! and each said the same thing in its crate doc: *"the controller REST login,
//! the WebSocket event transport and the actual API calls are network-bound and
//! deferred to Phase 1b; they feed their wire formats onto the models in this
//! crate and reuse the engine unchanged."* **This crate is that Phase 1b.**
//!
//! It is the single console client a household's UniFi stack actually needs:
//!
//! - [`transport`] — the async [`HttpTransport`] seam, a real `reqwest` +
//!   `rustls` [`transport::ReqwestTransport`] (self-signed-cert tolerant, as
//!   every UniFi OS console ships one), and a [`transport::MockTransport`] for
//!   fast offline unit tests.
//! - [`console`] — the [`console::Console`] abstraction over a **Cloud Key /
//!   UniFi OS console** (Dream Machine, UNVR, Cloud Key Gen2+) versus a
//!   **legacy** standalone Network controller, which differ only in URL prefix
//!   (`/proxy/network/...` vs direct `:8443`).
//! - [`auth`] — session auth: username + password login (legacy `/api/login`
//!   and UniFi OS `/api/auth/login`, capturing the `TOKEN` cookie + the
//!   `x-csrf-token`), and the newer **API-key** header mode.
//! - [`network`] — the Network Controller REST surface: sites, clients,
//!   devices, port stats and the event log, mapped onto
//!   [`cave_home_unifi_network`] types.
//! - [`access`] — the UniFi Access developer REST + notification WebSocket:
//!   doors, visitors, access events and **intercom unlock**, mapped onto
//!   [`cave_home_unifi_access`] types.
//! - [`protect`] — the UniFi Protect REST bootstrap + binary update WebSocket:
//!   cameras, recordings and the **live RTSPS stream URL**, mapped onto
//!   [`cave_home_unifi_protect`] types.
//! - [`ws`] — the real-time [`tokio_tungstenite`] WebSocket subscription engine
//!   shared by all three pillars.
//! - [`metrics`] — the Prometheus exposition for the console client.
//! - [`render`] — the grandma-friendly EN/DE/TR rendering used by the CLI track.
//!
//! Per Charter §9 only the **local** console API is ever targeted — there is no
//! Ubiquiti-cloud dependency in any path here.

pub mod console;
pub mod error;
pub mod transport;

pub use console::{Console, ConsoleKind};
pub use error::{Result, UnifiError};
pub use transport::{HttpMethod, HttpRequest, HttpResponse, HttpTransport, MockTransport};
