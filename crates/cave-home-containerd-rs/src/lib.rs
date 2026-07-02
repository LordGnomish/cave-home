// SPDX-License-Identifier: Apache-2.0
//! cave-home-containerd-rs — the CRI/OCI container-runtime **decision core**
//! for the cave-home K3s-style orchestration layer (ADR-004, Orchestration
//! Phase 1).
//!
//! This crate is **infrastructure** (Charter §6.3): it is hidden from
//! end-users, produces no user-facing strings, and carries no i18n. Its only
//! product is correctness.
//!
//! # Scope
//!
//! It is a *behavioural reimplementation* of the documented containerd / CRI /
//! OCI decision logic — std-only, no async runtime, no gRPC stack, no network
//! client. The deferred I/O layer (the runc/shim exec + syscalls, the gRPC CRI
//! server, the registry pull/distribution client, the overlayfs snapshotter,
//! the cgroup/namespace syscalls) is enumerated in `parity.manifest.toml`.
//!
//!   * [`digest`]    — content-addressable [`Digest`](digest::Digest) +
//!     [`Descriptor`](digest::Descriptor) (OCI content model).
//!   * [`reference`] — OCI/Docker image-reference parsing + normalisation.
//!   * [`oci`]       — OCI runtime-spec (`config.json`) construction.
//!   * [`lifecycle`] — container/task lifecycle state machine.
//!   * [`cri`]       — CRI PodSandbox/Container status, image-pull policy,
//!     unused-image GC selection.
//!   * [`snapshot`]  — overlayfs / native snapshotter mount assembly
//!     (`lowerdir`/`upperdir`/`workdir` decision; bind-vs-overlay choice).
//!
//! # Example
//!
//! Parse and normalise an image reference, then decide whether to pull it:
//!
//! ```
//! use cave_home_containerd_rs::reference::Reference;
//! use cave_home_containerd_rs::cri::pull_policy::{decide_pull, PullPolicy, PullDecision};
//!
//! let image = Reference::parse("nginx").expect("valid reference");
//! assert_eq!(image.canonical(), "docker.io/library/nginx:latest");
//!
//! // Image not present locally, IfNotPresent policy -> pull it.
//! assert_eq!(decide_pull(PullPolicy::IfNotPresent, false), PullDecision::Pull);
//! ```

pub mod cri;
pub mod digest;
pub mod lifecycle;
pub mod oci;
pub mod reference;
pub mod snapshot;
