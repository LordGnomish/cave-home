// SPDX-License-Identifier: Apache-2.0
//! cave-home-cni-flannel — Rust line-by-line port of flannel-io/flannel
//! `v0.28.4` (commit `3adfe3e0`). Apache-2.0 upstream → Apache-2.0 here.
//!
//! Phase 1 MVP scope (per ADR-008 + ROADMAP M2):
//!
//! - [`subnet`]   — lease manager + registry abstraction (in-mem + etcd).
//! - [`backend`]  — `Backend` trait + VXLAN datapath (Linux-only `flannel.<vni>`
//!                  device, netlink FDB/ARP install on lease events).
//! - [`cni`]      — CNI plugin protocol types + ADD/DEL/CHECK/VERSION handler
//!                  (driven by the `cave-home-cni-flannel` binary).
//! - [`config`]   — `NetworkConfig` (Network/SubnetLen/EnableIPv4/BackendType).
//!
//! Phase 1b backlog: host-gw, WireGuard, IPSec, multi-network CNI, Kubernetes
//! `FlannelNetwork` CRD watcher, delegate plugin chaining (bridge/portmap).
//! See `parity.manifest.toml` `[[unmapped]]` entries for the full list.

pub mod backend;
pub mod cni;
pub mod config;
pub mod subnet;
