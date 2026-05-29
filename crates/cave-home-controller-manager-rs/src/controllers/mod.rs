// SPDX-License-Identifier: Apache-2.0
//! Concrete controllers — pure reconcile/decision logic, no apiserver I/O.
//!
//! Each controller computes *what should change* (a delete set, a taint
//! decision, a finalizer sweep) over an in-memory view of objects. Performing
//! those changes against a live apiserver is the deferred client phase (see
//! `parity.manifest.toml`). The three shipped here are the smallest set that
//! exercises the three distinct controller shapes: graph-based (GC),
//! condition/heartbeat-based (node lifecycle), and finalizer/TTL-based
//! (cleanup).

pub mod cleanup;
pub mod gc;
pub mod node_lifecycle;
