// SPDX-License-Identifier: Apache-2.0
//! cave-home-controller-manager-rs — Kubernetes controller-manager.
//!
//! Line-by-line port of `pkg/controller/` from `kubernetes/kubernetes` v1.36.1
//! (SHA `756939600b9a7180fc2df6550a4585b638875e67`).
//!
//! Phase 2 MVP scope (per ADR-004, ROADMAP M3 — Orchestration Phase 2):
//! - [`types`]      — k8s API type subset (Deployment, ReplicaSet, DaemonSet,
//!                    StatefulSet, Job, CronJob, Namespace, Node, ServiceAccount,
//!                    OwnerReference + the [`types::KubeResource`] trait).
//! - [`api_client`] — [`api_client::ControllerApiClient`] trait + the
//!                    [`api_client::InMemoryApiClient`] test implementation.
//! - [`informer`]   — Shared informer pattern (port of `pkg/controller/informers`).
//! - [`workqueue`]  — Rate-limited workqueue (port of
//!                    `k8s.io/client-go/util/workqueue`).
//! - [`controllers`] — One module per Phase 2 MVP controller.
//! - [`manager`]    — Top-level [`manager::ControllerManager`] that registers
//!                    and runs every controller.
//!
//! Out of Phase 2 scope (see `parity.manifest.toml` `[[unmapped]]` entries
//! and the Phase 2b deferred list).

pub mod api_client;
pub mod controllers;
pub mod informer;
pub mod manager;
pub mod types;
pub mod workqueue;
