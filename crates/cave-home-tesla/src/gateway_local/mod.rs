// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall **local Gateway** surface — the on-LAN HTTPS API the gateway
//! exposes directly (community-documented, public).
//!
//! It is a low-latency fallback for when the Tesla cloud is unreachable: the
//! same [`PowerFlowData`](crate::models::PowerFlowData) can be read straight
//! from `/api/meters/aggregates` + `/api/system_status/soe`. The gateway serves
//! a self-signed certificate, so the real transport pins/ignores it — that TLS
//! detail is the Phase-1b transport's concern; the parsing + mapping here are
//! pure.

pub mod client;
pub mod types;

pub use client::{login_body, GatewayClient, LoginRequest};
pub use types::{MeterReading, MetersAggregates, SystemSoe};
