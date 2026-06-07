// SPDX-License-Identifier: Apache-2.0
//! Concrete controllers.
//!
//! Two shapes live here. The original three — graph-based (GC),
//! condition/heartbeat-based (node lifecycle) and finalizer/TTL-based
//! (cleanup) — are **pure decision functions**: they compute *what should
//! change* over an in-memory view and return it.
//!
//! The workload controllers (starting with [`replicaset`]) are **full
//! reconcilers**: each `reconcile`s one object key against the in-memory
//! apiserver ([`crate::apis::Cluster`]), reading the informer cache and issuing
//! create/update/delete writes — the real controller loop, with only the
//! network transport deferred.

pub mod cleanup;
pub mod deployment;
pub mod gc;
pub mod job;
pub mod node_lifecycle;
pub mod replicaset;
