// SPDX-License-Identifier: Apache-2.0
//! Backend-agnostic service-proxy *decision core*.
//!
//! This is the brain of kube-proxy expressed independently of any kernel
//! backend (iptables / ipvs / nftables): given the current Services and their
//! EndpointSlices, decide which endpoints receive traffic and emit a set of
//! abstract forwarding rules ([`rules::ProxyRule`]). A backend module then
//! lowers that IR to its wire format — the iptables lowering lives in
//! [`crate::iptables`].
//!
//! Modules:
//! * [`model`] — Service / EndpointSlice / Endpoint data model.
//! * [`validate`] — structural validation (reject malformed input, don't panic).
//! * [`select`] — endpoint selection: readiness, topology hints, traffic policy.
//! * [`rules`] — the `ProxyRule` IR, the rule generator, and the incremental diff.

pub mod model;
pub mod rules;
pub mod select;
pub mod validate;
