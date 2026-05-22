// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! cave-home-free-home — Busch-Jaeger free@home local-API client.
//!
//! Line-by-line port of `kingsleyadam/local-abbfreeathome` v3.5.1
//! (SHA `1f6e3ebcf448a07ad53b9cc4dbe64d013ba4cfee`, MIT-licensed). The
//! upstream is the actively-maintained Python library used by the Home
//! Assistant `freeathome` integration; the older Apache-2.0-implied
//! `Busch-Jaeger/free-at-home` org repository referenced by ADR-011 has
//! been superseded — `kingsleyadam/local-abbfreeathome` is the
//! community-maintained replacement, MIT-licensed and permissive
//! enough for line-by-line porting per Charter §6.1 + ADR-002.
//!
//! ## Architecture
//!
//! free@home is a residential REST/WebSocket facade on top of the KNX-IP
//! bus. The System Access Point (SysAP) exposes:
//!
//! 1. **REST endpoints** (`api`) — `GET /api/rest/configuration`,
//!    `PUT /api/rest/datapoint/<serial>/<channel>/<datapoint>`,
//!    `GET /api/rest/sysap`. Authentication is HTTP Basic; HTTPS is
//!    optional for SysAP firmware ≥ 2.6.
//! 2. **WebSocket endpoint** `wss://<sysap>/fhapi/v1/api/ws` — receives
//!    realtime datapoint deltas as JSON envelopes
//!    `{ "datapoints": { "ABB...../ch0001/odp0000": "1" } }`.
//!
//! 3. **Channel model** — devices contain channels, channels carry
//!    inputs/outputs/parameters, each input/output is identified by an
//!    integer "pairing ID" from the public free@home pairing table.
//!
//! ## Charter v6 / ADR-007 grandma-friendly UX
//!
//! Nothing here is grandma-facing. Device serials ("ABB7F500BCFB"),
//! channel ids ("ch0000"), datapoint ids ("idp0000") and pairing IDs
//! never leak past the Portal developer view. Grandma-facing labels
//! ("Mutfak Tavan Işığı") live in `cave-home-portal::admin::free_home`.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]

pub mod api;
pub mod channels;
pub mod device;
pub mod error;
pub mod freeathome;
pub mod pairing;
pub mod ws;
