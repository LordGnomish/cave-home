// SPDX-License-Identifier: Apache-2.0
//! cave-home-kubelet-rs — the node-side pod/container lifecycle decision core.
//!
//! Behavioural reimplementation of the documented kubelet pod-lifecycle
//! algorithms (Kubernetes "Pod Lifecycle" + "Configure Liveness/Readiness/
//! Startup Probes" + "Node-pressure Eviction" docs and the public behaviour of
//! `pkg/kubelet`). This is **not** a verbatim line-by-line transcription of a
//! specific upstream source revision — see `parity.manifest.toml` for the
//! honest port-method statement and the ADR-004 phase-1b deferral of the
//! verbatim-parity / CRI-runtime / cgroup / kubelet-server layers.
//!
//! Per ADR-004 this crate is **infrastructure**, hidden from end-users
//! (Charter §6.3): it surfaces no user-facing strings and carries no i18n.
//! Correctness is the product.
//!
//! ## Decision core (pure, `std`-only — exhaustively tested)
//! - [`phase`]      — pod-phase derivation from container states + restart policy.
//! - [`restart`]    — restart decision + CrashLoopBackOff backoff curve.
//! - [`probe`]      — liveness/readiness/startup threshold state machines + Ready gating.
//! - [`sync`]       — desired-vs-observed action diff (start/kill).
//! - [`eviction`]   — QoS classification + resource-pressure eviction ranking.
//! - [`admission`]  — node resource accounting + pod admission ("does it fit?").
//! - [`resources`]  — minimal CPU(milli)/memory(byte) resource model.
//!
//! ## syncPod execution slice (async, CRI-backed)
//! - [`api`]   — k8s API type subset (Pod / Container / Volume / Status).
//! - [`cri`]   — CRI v1 client trait + in-memory mock.
//! - [`pleg`]  — Pod Lifecycle Event Generator (relist + diff).
//! - [`podworker`] — per-pod state machine that reconciles desired -> actual.
//! - [`volume`] — emptyDir + hostPath volume plugins + reconciler.
//! - [`status`] — pod-status manager (queue / dedup / retry).
//! - [`kubelet`] — top-level composition / `sync_pod` entry point.
//!
//! Out of Phase 1 scope (verbatim-parity, real CRI runtime exec, cgroup +
//! eviction-manager syscalls, the kubelet HTTP server, volume managers, device
//! plugins) — see `parity.manifest.toml` `[[unmapped]]`.
//!
//! # Example
//!
//! Derive a pod phase from its container statuses — the core kubelet decision:
//!
//! ```
//! use cave_home_kubelet_rs::api::{
//!     ContainerState, ContainerStateTerminated, ContainerStatus, PodPhase, RestartPolicy,
//! };
//! use cave_home_kubelet_rs::phase::derive_phase;
//!
//! let statuses = vec![ContainerStatus {
//!     name: "app".into(),
//!     state: ContainerState::Terminated(ContainerStateTerminated {
//!         exit_code: 0,
//!         ..Default::default()
//!     }),
//!     ..Default::default()
//! }];
//! // RestartPolicy::Never + a clean exit -> the pod Succeeded.
//! assert_eq!(derive_phase(&statuses, 1, RestartPolicy::Never), PodPhase::Succeeded);
//! ```

pub mod admission;
pub mod api;
pub mod cgroup;
pub mod container_gc;
pub mod cri;
pub mod eviction;
pub mod kubelet;
pub mod phase;
pub mod pleg;
pub mod podworker;
pub mod probe;
pub mod resources;
pub mod restart;
pub mod status;
pub mod sync;
pub mod volume;
