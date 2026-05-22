// SPDX-License-Identifier: Apache-2.0
//! `cave-home-kube-proxy-rs` — line-by-line port of upstream
//! `kubernetes/kubernetes` `pkg/proxy/iptables` (Phase 1 MVP).
//!
//! Upstream: kubernetes/kubernetes @ v1.36.1
//! SHA: `756939600b9a7180fc2df6550a4585b638875e67`
//! Subpath: `pkg/proxy/iptables`
//!
//! Phase 1 scope (ClusterIP only, iptables mode):
//! - Pure rule generator (`syncProxyRules`)
//! - Service / EndpointSlice event-driven cache
//! - `iptables-restore` Linux executor (mockable trait)
//! - Debounced reconciler loop
//!
//! See `parity.manifest.toml` for `[[unmapped]]` Phase 1b backlog.

pub mod api;
pub mod cache;
pub mod iptables;
pub mod proxier;
