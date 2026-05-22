// SPDX-License-Identifier: Apache-2.0
//! cave-home-kubelet-rs — node-side agent.
//!
//! Line-by-line port of `pkg/kubelet/` from `kubernetes/kubernetes` v1.36.1
//! (SHA `756939600b9a7180fc2df6550a4585b638875e67`).
//!
//! Phase 1 MVP scope (per ADR-004, ROADMAP M2.5):
//! - [`api`]   — k8s API type subset (Pod / Container / Volume / Status).
//! - [`cri`]   — CRI v1 client trait + in-memory mock.
//! - [`pleg`]  — Pod Lifecycle Event Generator (relist + diff).
//! - [`podworker`] — per-pod state machine that reconciles desired -> actual.
//! - [`volume`] — emptyDir + hostPath volume plugins + reconciler.
//! - [`status`] — pod-status manager (queue / dedup / retry).
//! - [`kubelet`] — top-level composition / `sync_pod` entry point.
//!
//! Out of Phase 1 scope (see `parity.manifest.toml` `[[unmapped]]` entries).

pub mod api;
pub mod cri;
pub mod kubelet;
pub mod pleg;
pub mod podworker;
pub mod status;
pub mod volume;
