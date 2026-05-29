// SPDX-License-Identifier: Apache-2.0
//! CRI (Container Runtime Interface) decision semantics.
//!
//! Behavioural reimplementation of the documented Kubernetes CRI v1 model:
//! the `PodSandbox` + `Container` state model, the CRI status computation, the
//! kubelet image-pull-policy decision, and unused-image garbage collection.
//! Pure decision logic — the gRPC `RuntimeService`/`ImageService` server,
//! the kubelet wiring and the runc exec are deferred (see
//! `parity.manifest.toml`).
//!
//! Spec sources:
//!   * Kubernetes CRI API `runtime/v1/api.proto` — `PodSandboxState`,
//!     `ContainerState`, `PodSandboxStatus`, `ContainerStatus`.
//!   * Kubernetes image-pull-policy semantics (`Always` / `IfNotPresent` /
//!     `Never`) as documented for the kubelet image manager.
//!   * containerd CRI image GC: images not referenced by any container are
//!     eligible for removal.

pub mod gc;
pub mod pull_policy;
pub mod status;

pub use gc::{ImageRecord, select_unused_images};
pub use pull_policy::{PullDecision, PullPolicy, decide_pull};
pub use status::{
    Container, ContainerState, PodSandbox, SandboxState, container_status, sandbox_status,
};
