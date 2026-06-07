// SPDX-License-Identifier: Apache-2.0
//! `cave-home-controller-manager-rs` — the controller-framework **decision
//! core** for cave-home's K3s-style orchestration layer (ADR-004).
//!
//! This is **infrastructure**: it is hidden from end users (Charter §6.3), so
//! it carries no user-facing strings and no i18n — correctness is the only
//! product. It is a `std`-only, panic-free library: every decision is pure over
//! data the caller supplies, including the clock (a monotonic `now`), and all
//! state lives in an **in-memory apiserver** ([`apis::Cluster`]) the
//! controllers read and write — the client-go `fake.Clientset`/`ObjectTracker`
//! analogue, not a stub. The core workload controllers are implemented as real
//! reconcilers ([`controllers`]), driven by a [`manager`] run loop over
//! per-controller work queues, with lease-based [`leaderelection`]. Only the
//! *networked* transport (the REST clientset + watch reflector that would back
//! the cache over the wire) and the secondary controllers (`EndpointSlice`, HPA,
//! quota, volumes, certificates, cloud) remain **deferred** (see
//! `parity.manifest.toml` `[[unmapped]]`).
//!
//! ## Port method (honest)
//!
//! This is a **behavioural reimplementation** of the *documented* contracts of
//! the Kubernetes controller machinery — the client-go work queue + rate
//! limiter, the controller-runtime reconcile `Result`, the `tools/cache`
//! `Store`/`DeltaFIFO`, the core workload controllers, the manager run loop, and
//! lease-based leader election. It is **not** a verbatim line-by-line
//! transcription of unread Go source; the manifest names the behavioural
//! reference for each item, not a byte-for-byte claim.
//!
//! ## Modules
//!
//! - [`types`] — the minimal object model ([`types::ObjectMeta`],
//!   [`types::OwnerReference`], the [`types::Object`] trait).
//! - [`apis`] — the typed workload object model + the in-memory apiserver
//!   ([`apis::Cluster`], [`apis::Api`]).
//! - [`workqueue`] — rate-limited delaying work queue: dedup, per-key
//!   exponential backoff, max-retries-drop, `add_after`.
//! - [`reconcile`] — the [`reconcile::Reconciler`] trait, its
//!   [`reconcile::Outcome`], and the pure loop decision.
//! - [`informer`] — [`informer::Store`] indexer + [`informer::DeltaFifo`].
//! - [`controllers`] — the concrete controllers: workload reconcilers
//!   (`ReplicaSet`, `Deployment`, `StatefulSet`, `DaemonSet`, `Job`, `CronJob`,
//!   `Namespace`, `ServiceAccount`, `Endpoints`) plus the pure GC /
//!   node-lifecycle / cleanup decision functions.
//! - [`manager`] — the run loop wiring controllers to work queues.
//! - [`leaderelection`] — lease-based single-active-manager election.
//!
//! ## Example
//!
//! Drive the work queue's dedup + backoff decision directly:
//!
//! ```
//! use cave_home_controller_manager_rs::workqueue::{WorkQueue, RateLimitConfig, AddOutcome};
//!
//! let mut q = WorkQueue::new(RateLimitConfig { base_delay: 10, max_delay: 1000, max_retries: 5 });
//!
//! // Dedup: the same key enqueued twice is processed once.
//! q.add("prod/web");
//! q.add("prod/web");
//! assert_eq!(q.ready_len(), 1);
//!
//! // Process it, then requeue with backoff after a failure (caller supplies `now`).
//! let key = q.get(0).expect("a ready item");
//! q.done(&key);
//! let outcome = q.add_rate_limited(&key, 0);
//! assert_eq!(outcome, AddOutcome::Requeued { failures: 1, delay: 10 });
//! ```

pub mod apis;
pub mod controllers;
pub mod informer;
pub mod leaderelection;
pub mod manager;
pub mod reconcile;
pub mod types;
pub mod workqueue;
