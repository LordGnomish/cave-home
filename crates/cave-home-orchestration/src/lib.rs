//! `cave-home-orchestration` — the in-process K3s-orchestration bootstrap +
//! component-coordination **decision core** (ADR-004, Charter §5).
//!
//! ADR-004 settled cave-home's orchestration shape: a behavioural
//! reimplementation of K3s that compiles into the **one** cave-home binary
//! (the Charter §5 single-binary carve-out was explicitly *not* taken). This
//! crate is the pure-logic brain of that layer — the wiring **decisions** that
//! bring the K3s control plane and node-side components up *in process*:
//!
//! - **Node-role config** — server vs agent, datastore (embedded kine / `SQLite`
//!   vs external), the cluster/node token model (shape-validated, no crypto),
//!   cluster-init vs join, and the flannel / servicelb / traefik flags +
//!   disable list — [`config`].
//! - **Component model** — the fixed set of in-process components and their
//!   documented start-up dependencies — [`component`].
//! - **Bring-up ordering** — a deterministic topological start order with
//!   cycle / missing-dependency detection and readiness gating — [`bringup`].
//! - **Bootstrap state machine** — `Initializing -> DatastoreReady ->
//!   ApiserverReady -> ControlPlaneReady -> NodeReady -> Running`, with
//!   transient-vs-fatal failure classification and per-step retry budget —
//!   [`bootstrap`].
//! - **Single-binary invariant** — the model asserts every component is
//!   in-process (no sidecar / sub-process), preserving Charter §5 —
//!   [`component::all_in_process`].
//! - **Multi-node role assignment** — maps homeowner intent (primary / backup
//!   hub, worker) onto the server/agent role + cluster-start decision,
//!   independent of `cave-home-cluster` — [`role`].
//! - **Graceful shutdown** — the bring-up order, reversed, with a safety
//!   predicate — [`shutdown`].
//! - **Secrets encryption-at-rest** — the K3s / Kubernetes envelope-encryption
//!   decision core with a post-quantum envelope (ML-KEM-768 + AES-256-GCM): the
//!   write-key/read-keys keyring + rotation lifecycle, the in-process KMS
//!   provider, the prefixed stored-value transformer + identity fallback, the
//!   encryption-configuration model, and the status/observability contracts —
//!   [`secrets_encryption`] (ADR-033).
//!
//! # Scope (honest)
//!
//! This crate is **pure logic**: no process supervision, no network, no clock
//! (the caller drives the state machine). It is std-and-crypto-only — the one
//! crypto exception is [`secrets_encryption`], whose at-rest envelope genuinely
//! does ML-KEM-768 + AES-256-GCM (ADR-033 PQC-ready mandate); the rest of the
//! crate touches no crypto. It models the *decisions* K3s's server/agent
//! bootstrap makes — the order, the gating, the role split, the failure
//! classification — reimplemented from the public K3s architecture
//! documentation, **not** transcribed verbatim from Go source.
//! The actual in-process component supervision, the kine/etcd datastore
//! runtime, TLS bootstrap + cert rotation, the node-join handshake transport,
//! the containerd/CNI runtime, the embedded registry, and the live apiserver
//! storage wiring + gRPC KMS-plugin transport that would feed
//! [`secrets_encryption`] a running keyring are all network / runtime-bound and
//! are ADR-004 phase-1b — every one is enumerated in `parity.manifest.toml`
//! `[[unmapped]]`.
//!
//! This is **infrastructure**, hidden from end users (Charter §6.3, ADR-007):
//! it surfaces no user-facing strings and carries no i18n.
//!
//! # Example
//!
//! Plan a single-node primary hub's bring-up and walk its bootstrap to running:
//!
//! ```
//! use cave_home_orchestration::{
//!     bringup::BringUpPlan,
//!     bootstrap::{Bootstrap, Phase},
//!     component::Component,
//!     role::{NodeIntent, validate_cluster},
//! };
//!
//! // A one-node cluster: just the primary hub.
//! let intents = [NodeIntent::PrimaryHub];
//! validate_cluster(&intents).expect("exactly one primary");
//!
//! // The primary hub runs the full component set; plan a dependency-correct
//! // start order (kine first, kube-proxy last among networking).
//! let components = NodeIntent::PrimaryHub.components();
//! let plan = BringUpPlan::compute(&components).expect("acyclic, self-contained");
//! assert_eq!(plan.order()[0], Component::Kine);
//!
//! // Single-binary invariant: every component runs in-process (Charter §5).
//! assert!(cave_home_orchestration::component::all_in_process(plan.order()));
//!
//! // Drive the bootstrap state machine to steady state.
//! let mut boot = Bootstrap::default();
//! while !boot.is_running() {
//!     boot.succeed();
//! }
//! assert_eq!(boot.phase(), Phase::Running);
//!
//! // Shutdown is the exact reverse — the datastore is torn down last.
//! assert_eq!(plan.shutdown_order().last(), Some(&Component::Kine));
//! ```

pub mod bootstrap;
pub mod bringup;
pub mod component;
pub mod config;
pub mod local_path_provisioner;
pub mod metrics_server;
pub mod role;
pub mod secrets_encryption;
pub mod shutdown;

pub use bootstrap::{Bootstrap, FailureKind, Phase, Transition};
pub use bringup::{BringUpPlan, OrderError};
pub use component::{Component, all_in_process};
pub use config::{
    Addons, AgentConfig, ClusterStart, ConfigError, Datastore, NodeConfig, ServerConfig, Token,
};
pub use role::{NodeIntent, OrchestrationRole, RoleError, validate_cluster};
pub use shutdown::{is_safe_shutdown, shutdown_order, shutdown_order_with_external};
