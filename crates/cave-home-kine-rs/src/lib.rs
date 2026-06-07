// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-kine-rs` — the kine decision core: an etcd-MVCC key/value model
//! backed (in production) by SQL, the trick that lets a Kubernetes control
//! plane fit in one binary (ADR-004, Orchestration Phase 3).
//!
//! # What this is
//!
//! kine ("kine is not etcd") presents the etcd v3 API to the Kubernetes
//! apiserver while storing everything in a single SQL table. This crate is the
//! **decision core** of that translation: the revisioned key/value semantics
//! etcd guarantees, implemented as pure, `std`-only logic over an in-memory row
//! log so they can be tested exhaustively without a database or a network.
//!
//! * [`store`] — the append-only, revisioned row log: `create` / `update` /
//!   `put` / `delete` (tombstones) and the current-state view.
//! * [`revision`] — the monotonic global revision (etcd's MVCC clock).
//! * [`range`] — the etcd `Range` RPC: point get, prefix scan, `[key, end)`
//!   intervals, historical reads at a past revision, and `limit`.
//! * [`compact`] — compaction and the `"mvcc: required revision has been
//!   compacted"` read guard.
//! * [`watch`] — the ordered `PUT` / `DELETE` event stream for a key or prefix
//!   after a start revision.
//! * [`lease`] — lease/TTL attachment and the expiry decision (caller-supplied
//!   `now`) that deletes leased keys.
//! * [`sql`] — the SQL row schema + query templates kine *would* issue,
//!   modelled as typed Rust and **never executed**.
//!
//! # Honest port method (Charter §6)
//!
//! This is a **behavioural reimplementation** of documented etcd-MVCC and kine
//! semantics from public sources (the etcd API reference / MVCC design and the
//! Apache-2.0 `k3s-io/kine` backend). It is **not** a verbatim line-by-line
//! port, and it is labelled as such in the parity manifest. The real SQL driver
//! (`SQLite` / `Postgres` / `MySQL` / dqlite), transactions / row-locking, the
//! etcd gRPC server, TLS, and the apiserver wiring are **deferred to Phase-1b** —
//! they are I/O and protocol shells around exactly the logic implemented here.
//!
//! # Charter §6.3
//!
//! kine is pure infrastructure, hidden from the homeowner. This crate produces
//! **no user-facing strings** — its errors model etcd wire vocabulary for the
//! (future) gRPC layer, never the Portal.
//!
//! # Example
//!
//! ```
//! use cave_home_kine_rs::{Store, RangeRequest, execute, watch, EventKind};
//!
//! let mut store = Store::new();
//! store.create(b"/registry/pods/a", b"v1", 0).unwrap(); // revision 1
//! store.update(b"/registry/pods/a", b"v2", 0).unwrap(); // revision 2
//! store.create(b"/registry/pods/b", b"v1", 0).unwrap(); // revision 3
//!
//! // Prefix scan of the current state.
//! let resp = execute(&store, &RangeRequest::prefix(b"/registry/pods/")).unwrap();
//! assert_eq!(resp.count, 2);
//! assert_eq!(resp.revision, 3);
//!
//! // Historical read: what did key `a` hold at revision 1?
//! let past = execute(&store, &RangeRequest::key(b"/registry/pods/a").at_revision(1)).unwrap();
//! assert_eq!(past.kvs[0].value, b"v1");
//!
//! // Watch the change stream from the beginning.
//! let events = watch(&store, &RangeRequest::prefix(b"/registry/"), 0).unwrap();
//! assert_eq!(events.len(), 3);
//! assert_eq!(events[0].kind, EventKind::Put);
//! ```

#![forbid(unsafe_code)]
// A row count fits an i64 for any store a single binary will ever hold; the
// etcd revision/count fields are i64 to match the wire protocol.
#![allow(clippy::cast_possible_wrap)]

pub mod compact;
pub mod error;
pub mod lease;
pub mod range;
pub mod revision;
pub mod sql;
pub mod store;
pub mod txn;
pub mod watch;

pub use compact::{compact, CompactReport};
pub use error::{KineError, Result};
pub use lease::{Lease, LeaseTable, UnixSeconds};
pub use range::{execute, prefix_successor, RangeEnd, RangeRequest, RangeResponse};
pub use revision::{Clock, Revision, CURRENT};
pub use store::{Row, Store};
pub use watch::{watch, EventKind, WatchEvent};
