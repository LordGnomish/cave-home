// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The etcd gRPC transport — the wire API the Kubernetes apiserver speaks.
//!
//! kine's reason to exist is that it answers etcd's gRPC API while storing
//! everything in SQL. This module is that server: it implements the etcd `KV`
//! and `Maintenance` services (generated from the vendored
//! [`proto/rpc.proto`](../../proto/rpc.proto) subset) on top of the real
//! [`crate::sqlite::SqliteStore`], translating each etcd RPC into the
//! corresponding backend operation and the result back into etcd wire messages.
//! An unmodified apiserver / `etcdctl` can talk to it.
//!
//! The store is held behind a [`tokio::sync::Mutex`]: kine serialises writes
//! (one global revision sequence), so a single guarded connection is the honest
//! model, not a bottleneck for a home-scale control plane.
//!
//! Reference: etcd `api/etcdserverpb/rpc.proto` services `KV` / `Maintenance`
//! and kine `pkg/server` request handlers (`Range` / `Put` / `DeleteRange` /
//! `Txn` / `Compact`). Faithful behavioural port, Apache-2.0.

#![cfg(feature = "grpc")]
// tonic idioms: `Status` is a large error type (intrinsic to tonic), and each
// handler intentionally holds the store `MutexGuard` for the whole operation to
// keep kine's single global-revision sequence serialised.
#![allow(clippy::result_large_err, clippy::significant_drop_tightening)]

use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio_stream::Stream;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::error::KineError;
use crate::lease::{LeaseTable, UnixSeconds};
use crate::range::{prefix_successor, RangeEnd, RangeRequest as KineRange, RangeResponse as KineRangeResp};
use crate::metrics::KineMetrics;
use crate::sqlite::SqliteStore;
use crate::store::Row;
use crate::watch::{EventKind, WatchEvent};

/// The generated etcd protobuf types and service stubs.
pub mod etcdserverpb {
    #![allow(clippy::all, clippy::pedantic, clippy::nursery, missing_docs)]
    tonic::include_proto!("etcdserverpb");
}

use etcdserverpb::{
    kv_server::{Kv, KvServer},
    lease_server::{Lease, LeaseServer},
    maintenance_server::{Maintenance, MaintenanceServer},
    watch_request, watch_server::{Watch, WatchServer},
    request_op, response_op, CompactionRequest, CompactionResponse, Compare, DeleteRangeRequest,
    DeleteRangeResponse, Event, KeyValue, LeaseGrantRequest, LeaseGrantResponse,
    LeaseKeepAliveRequest, LeaseKeepAliveResponse, LeaseRevokeRequest, LeaseRevokeResponse,
    LeaseTimeToLiveRequest, LeaseTimeToLiveResponse, PutRequest, PutResponse, RangeRequest,
    RangeResponse, RequestOp, ResponseHeader, ResponseOp, StatusRequest, StatusResponse, TxnRequest,
    TxnResponse, WatchCreateRequest, WatchRequest, WatchResponse,
};

/// A pluggable wall-clock source for lease TTLs, in whole Unix seconds. Injected
/// (rather than reading the system clock inline) so lease expiry and keep-alive
/// renewal are deterministically testable.
pub type ClockFn = Arc<dyn Fn() -> UnixSeconds + Send + Sync>;

/// The default lease clock: the system wall clock truncated to whole seconds.
fn system_clock() -> UnixSeconds {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// How often a watch polls the backend for new revisions (kine's watch is a
/// poll over the after-query, not a push — this is that interval).
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How many consecutive idle poll ticks elapse before a `progress_notify` watch
/// emits a progress notification — throttles a quiet range to ~1 update / 300ms
/// rather than one every poll.
const PROGRESS_IDLE_TICKS: u32 = 3;

/// kine's reported etcd server version (the apiserver only checks it is a
/// 3.x etcd that supports the v3 API).
pub const ETCD_VERSION: &str = "3.5.13";

/// The control action a watch's poll loop takes after servicing its inbound
/// control stream — kept out of the `select!` arms so error propagation can use
/// `?` in the surrounding try-stream.
enum WatchCtl {
    /// Keep polling.
    Continue,
    /// The client asked to cancel watch `id`; emit the marker and stop.
    Cancel(i64),
    /// The inbound transport errored; surface it and stop.
    Failed(Status),
}

/// An etcd gRPC server backed by a real [`SqliteStore`]. Cheap to clone (the
/// store is shared behind an `Arc`), so the same instance can back several
/// service servers ([`Self::kv`], [`Self::maintenance`]).
#[derive(Clone)]
pub struct KineServer {
    store: Arc<Mutex<SqliteStore>>,
    metrics: Arc<KineMetrics>,
    /// The in-memory lease registry (`id -> ttl/granted_at`). kine tracks lease
    /// lifetime in the server, not in SQL; only the per-row `lease` column is
    /// persisted, so the table is rebuilt on restart from outstanding
    /// `KeepAlive`s.
    leases: Arc<Mutex<LeaseTable>>,
    /// Monotonic allocator for server-assigned lease ids (when a client grants
    /// with id `0`). Starts high so it never collides with the small ids tests
    /// and bootstrap code hand-pick.
    next_lease_id: Arc<AtomicI64>,
    /// The wall clock used for lease TTLs (injectable for tests).
    clock: ClockFn,
}

impl KineServer {
    /// Wrap an owned store with the default system clock.
    #[must_use]
    pub fn new(store: SqliteStore) -> Self {
        Self::with_clock(store, Arc::new(system_clock))
    }

    /// Wrap an already-shared store (so a watcher / metrics layer can share it).
    #[must_use]
    pub fn from_shared(store: Arc<Mutex<SqliteStore>>) -> Self {
        Self {
            store,
            metrics: Arc::new(KineMetrics::new()),
            leases: Arc::new(Mutex::new(LeaseTable::new())),
            next_lease_id: Arc::new(AtomicI64::new(1)),
            clock: Arc::new(system_clock),
        }
    }

    /// Wrap an owned store with an explicit lease clock. The clock returns the
    /// current time in whole Unix seconds; tests pass a controllable cell to
    /// drive lease expiry / keep-alive deterministically.
    #[must_use]
    pub fn with_clock(store: SqliteStore, clock: ClockFn) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            metrics: Arc::new(KineMetrics::new()),
            leases: Arc::new(Mutex::new(LeaseTable::new())),
            next_lease_id: Arc::new(AtomicI64::new(1)),
            clock,
        }
    }

    /// The shared store handle.
    #[must_use]
    pub fn store(&self) -> Arc<Mutex<SqliteStore>> {
        Arc::clone(&self.store)
    }

    /// The server's metric registry (Prometheus exposition via
    /// [`KineMetrics::render`]).
    #[must_use]
    pub fn metrics(&self) -> Arc<KineMetrics> {
        Arc::clone(&self.metrics)
    }

    /// This server as a tonic `KV` service, ready for `Server::add_service`.
    #[must_use]
    pub fn kv(&self) -> KvServer<Self> {
        KvServer::new(self.clone())
    }

    /// This server as a tonic `Maintenance` service.
    #[must_use]
    pub fn maintenance(&self) -> MaintenanceServer<Self> {
        MaintenanceServer::new(self.clone())
    }

    /// This server as a tonic `Watch` service.
    #[must_use]
    pub fn watch(&self) -> WatchServer<Self> {
        WatchServer::new(self.clone())
    }

    /// This server as a tonic `Lease` service.
    #[must_use]
    pub fn lease(&self) -> LeaseServer<Self> {
        LeaseServer::new(self.clone())
    }

    /// Drive a watch with no client control stream — historical replay for the
    /// in-process callers and tests. Equivalent to [`Self::watch_stream_with`]
    /// fed an inbound that never sends a cancel.
    ///
    /// etcd revision semantics: `start_revision == 0` means "future changes
    /// only"; a positive `start_revision` replays from that revision inclusive.
    pub fn watch_stream(
        &self,
        create: WatchCreateRequest,
    ) -> impl Stream<Item = Result<WatchResponse, Status>> + Send + use<> {
        self.watch_stream_with(create, tokio_stream::pending())
    }

    /// The event stream for one watch, honouring the client's control stream:
    /// a `created` marker, then ordered change events (carrying `prev_kv` when
    /// requested), periodic progress notifications when `progress_notify` is set
    /// and the range is idle, and a `canceled` marker (ending the stream) when
    /// the client sends a [`WatchCancelRequest`]. kine's watch is a poll over the
    /// after-query; this is that poll, made cancellable.
    pub fn watch_stream_with<I>(
        &self,
        create: WatchCreateRequest,
        inbound: I,
    ) -> impl Stream<Item = Result<WatchResponse, Status>> + Send + use<I>
    where
        I: Stream<Item = Result<WatchRequest, Status>> + Send + 'static,
    {
        use tokio_stream::StreamExt as _;
        let store = self.store();
        let want_prev = create.prev_kv;
        let progress_notify = create.progress_notify;
        async_stream::try_stream! {
            let filter = to_kine_range_bytes(&create.key, &create.range_end)?;
            let watch_id = create.watch_id;
            tokio::pin!(inbound);

            let revision = {
                let s = store.lock().await;
                s.current_revision().map_err(status)?
            };
            yield watch_response(watch_id, revision, true, Vec::new());

            // watch_after is exclusive (mod_revision > last); translate etcd's
            // inclusive start_revision, and "0 = from now" to the current head.
            let mut last = if create.start_revision > 0 {
                create.start_revision - 1
            } else {
                revision
            };
            let mut inbound_open = true;
            let mut idle_ticks = 0u32;
            loop {
                let (events, header_rev) = {
                    let s = store.lock().await;
                    let evs = s.watch_after(&filter, last).map_err(status)?;
                    (evs, s.current_revision().map_err(status)?)
                };
                if !events.is_empty() {
                    last = events.last().map_or(last, |e| e.revision);
                    let proto = events.iter().map(|e| to_event(e, want_prev)).collect();
                    yield watch_response(watch_id, header_rev, false, proto);
                    idle_ticks = 0;
                } else if progress_notify {
                    // An idle progress tick: an empty response carrying the head
                    // revision so the watcher can advance its checkpoint without
                    // a change event. Throttled so a quiet range is not chatty.
                    idle_ticks += 1;
                    if idle_ticks >= PROGRESS_IDLE_TICKS {
                        idle_ticks = 0;
                        yield watch_response(watch_id, header_rev, false, Vec::new());
                    }
                }

                // Pace the poll, but wake early to service a cancel request. The
                // `?`-propagation must live outside the select! arms, so the arm
                // only classifies the inbound message into a control action.
                let ctl = tokio::select! {
                    biased;
                    maybe = inbound.next(), if inbound_open => match maybe {
                        Some(Ok(WatchRequest {
                            request_union: Some(watch_request::RequestUnion::CancelRequest(c)),
                        })) => WatchCtl::Cancel(if c.watch_id != 0 { c.watch_id } else { watch_id }),
                        // A stray create / empty union: ignore (one watch per stream).
                        Some(Ok(_)) => WatchCtl::Continue,
                        Some(Err(e)) => WatchCtl::Failed(e),
                        // Client half-closed its send side: stop listening but
                        // keep streaming events (etcd keeps the watch alive).
                        None => { inbound_open = false; WatchCtl::Continue }
                    },
                    () = tokio::time::sleep(WATCH_POLL_INTERVAL) => WatchCtl::Continue,
                };
                match ctl {
                    WatchCtl::Cancel(id) => {
                        yield watch_canceled(id, header_rev);
                        return;
                    }
                    WatchCtl::Failed(e) => { Err::<(), _>(e)?; }
                    WatchCtl::Continue => {}
                }
            }
        }
    }

    /// Build a response header stamped with the store's current revision.
    async fn header(&self) -> Result<ResponseHeader, Status> {
        let rev = {
            let store = self.store.lock().await;
            store.current_revision().map_err(status)?
        };
        Ok(ResponseHeader { cluster_id: 0, member_id: 0, revision: rev, raft_term: 0 })
    }

    /// Execute a [`RangeRequest`] under the lock and shape the etcd response.
    async fn do_range(&self, req: &RangeRequest) -> Result<RangeResponse, Status> {
        let kreq = to_kine_range(req)?;
        let store = self.store.lock().await;
        let resp = store.range(&kreq).map_err(status)?;
        Ok(shape_range(req, &resp, resp.revision))
    }

    /// Execute a put under the lock, optionally capturing the previous kv.
    async fn do_put(&self, req: &PutRequest) -> Result<PutResponse, Status> {
        if req.key.is_empty() {
            return Err(Status::new(Code::InvalidArgument, "etcdserver: key is not provided"));
        }
        let mut store = self.store.lock().await;

        // Fetch the current row first when prev_kv / ignore_* needs it.
        let prev = if req.prev_kv || req.ignore_value || req.ignore_lease {
            store
                .range(&KineRange::key(&req.key))
                .map_err(status)?
                .kvs
                .into_iter()
                .next()
        } else {
            None
        };
        let value = if req.ignore_value {
            prev.as_ref().map(|r| r.value.clone()).unwrap_or_default()
        } else {
            req.value.clone()
        };
        let lease = if req.ignore_lease {
            prev.as_ref().map_or(0, |r| r.lease)
        } else {
            req.lease
        };

        store.put(&req.key, &value, lease).map_err(status)?;
        let revision = store.current_revision().map_err(status)?;
        Ok(PutResponse {
            header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
            prev_kv: if req.prev_kv { prev.as_ref().map(row_to_kv) } else { None },
        })
    }

    /// Execute a delete-range under the lock: tombstone every live key in the
    /// interval, counting deletions and (optionally) returning prev kvs.
    async fn do_delete_range(&self, req: &DeleteRangeRequest) -> Result<DeleteRangeResponse, Status> {
        let selector = to_kine_range_bytes(&req.key, &req.range_end)?;
        let mut store = self.store.lock().await;
        let victims = store.range(&selector).map_err(status)?.kvs;

        let mut deleted = 0_i64;
        let mut prev_kvs = Vec::new();
        for row in &victims {
            if store.delete(&row.key).map_err(status)?.is_some() {
                deleted += 1;
                if req.prev_kv {
                    prev_kvs.push(row_to_kv(row));
                }
            }
        }
        let revision = store.current_revision().map_err(status)?;
        Ok(DeleteRangeResponse {
            header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
            deleted,
            prev_kvs,
        })
    }
}

#[tonic::async_trait]
impl Kv for KineServer {
    async fn range(&self, request: Request<RangeRequest>) -> Result<Response<RangeResponse>, Status> {
        let start = Instant::now();
        let result = self.do_range(request.get_ref()).await;
        self.metrics.record_request("range", start.elapsed().as_secs_f64(), result.is_ok());
        result.map(Response::new)
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let start = Instant::now();
        let result = self.do_put(request.get_ref()).await;
        self.metrics.record_request("put", start.elapsed().as_secs_f64(), result.is_ok());
        result.map(Response::new)
    }

    async fn delete_range(
        &self,
        request: Request<DeleteRangeRequest>,
    ) -> Result<Response<DeleteRangeResponse>, Status> {
        let start = Instant::now();
        let result = self.do_delete_range(request.get_ref()).await;
        self.metrics.record_request("delete", start.elapsed().as_secs_f64(), result.is_ok());
        result.map(Response::new)
    }

    async fn txn(&self, request: Request<TxnRequest>) -> Result<Response<TxnResponse>, Status> {
        let start = Instant::now();
        let result = self.do_txn(request.into_inner()).await;
        self.metrics.record_request("txn", start.elapsed().as_secs_f64(), result.is_ok());
        result.map(Response::new)
    }

    async fn compact(
        &self,
        request: Request<CompactionRequest>,
    ) -> Result<Response<CompactionResponse>, Status> {
        let start = Instant::now();
        let result = self.do_compact(request.into_inner()).await;
        self.metrics.record_request("compact", start.elapsed().as_secs_f64(), result.is_ok());
        result.map(Response::new)
    }
}

impl KineServer {
    /// Run a transaction: evaluate every comparison against the current
    /// committed state and run the success branch iff all hold, else the
    /// failure branch.
    async fn do_txn(&self, txn: TxnRequest) -> Result<TxnResponse, Status> {
        let succeeded = {
            let store = self.store.lock().await;
            let mut all = true;
            for cmp in &txn.compare {
                if !eval_compare(&store, cmp).map_err(status)? {
                    all = false;
                    break;
                }
            }
            all
        };
        let ops = if succeeded { txn.success } else { txn.failure };
        let mut responses = Vec::with_capacity(ops.len());
        for op in ops {
            responses.push(self.exec_op(op).await?);
        }
        Ok(TxnResponse { header: Some(self.header().await?), succeeded, responses })
    }

    /// Compact the store and record the rows removed into the metrics.
    async fn do_compact(&self, req: CompactionRequest) -> Result<CompactionResponse, Status> {
        let (revision, removed) = {
            let mut store = self.store.lock().await;
            let report = store.compact(req.revision).map_err(status)?;
            (store.current_revision().map_err(status)?, report.removed)
        };
        self.metrics.record_compaction(removed as u64);
        Ok(CompactionResponse {
            header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
        })
    }

    /// Execute a single Txn request op (put / delete / range) and wrap the
    /// etcd `ResponseOp`.
    async fn exec_op(&self, op: RequestOp) -> Result<ResponseOp, Status> {
        let response = match op.request {
            Some(request_op::Request::RequestRange(r)) => {
                response_op::Response::ResponseRange(self.do_range(&r).await?)
            }
            Some(request_op::Request::RequestPut(p)) => {
                response_op::Response::ResponsePut(self.do_put(&p).await?)
            }
            Some(request_op::Request::RequestDeleteRange(d)) => {
                response_op::Response::ResponseDeleteRange(self.do_delete_range(&d).await?)
            }
            None => return Err(Status::new(Code::InvalidArgument, "empty txn op")),
        };
        Ok(ResponseOp { response: Some(response) })
    }
}

/// Configuration for the background maintenance loop ([`KineServer::spawn_compactor`]).
#[derive(Debug, Clone, Copy)]
pub struct CompactionConfig {
    /// How many of the most recent revisions to keep; older history is
    /// compacted away. kine keeps a rolling window so the datastore does not
    /// grow without bound.
    pub retain: i64,
    /// How often the loop runs (compact + lease reap).
    pub interval: Duration,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        // kine's defaults: a 5-minute interval, keeping the last ~1000 revisions.
        Self { retain: 1000, interval: Duration::from_secs(300) }
    }
}

impl KineServer {
    /// Compact away everything older than the most recent `retain` revisions.
    /// Computes the target floor as `current - retain` and compacts to it when
    /// that advances the existing floor; otherwise it is a no-op (`Ok(None)`).
    /// This is kine's rolling-window compaction, driven by a background loop or
    /// an operator.
    ///
    /// # Errors
    /// An etcd `Status` if the backend compaction fails.
    pub async fn compact_retaining(&self, retain: i64) -> Result<Option<crate::compact::CompactReport>, Status> {
        let (current, floor) = {
            let store = self.store.lock().await;
            (store.current_revision().map_err(status)?, store.compacted_revision().map_err(status)?)
        };
        let target = current - retain.max(0);
        if target <= 0 || target <= floor {
            return Ok(None);
        }
        let report = {
            let mut store = self.store.lock().await;
            store.compact(target).map_err(status)?
        };
        self.metrics.record_compaction(report.removed as u64);
        Ok(Some(report))
    }

    /// Reap every lease whose TTL has elapsed by "now": forget it and delete all
    /// keys attached to it (etcd's lessor tick). Returns the number of keys
    /// deleted. Time comes from the injected clock, so the loop is testable.
    ///
    /// # Errors
    /// An etcd `Status` if the backend deletion fails.
    pub async fn reap_expired_leases(&self) -> Result<usize, Status> {
        let now = (self.clock)();
        let expired = {
            let mut leases = self.leases.lock().await;
            let ids = leases.expired(now);
            for id in &ids {
                leases.revoke(*id);
            }
            ids
        };
        let mut store = self.store.lock().await;
        let mut deleted = 0;
        for id in expired {
            deleted += store.revoke_lease_keys(id).map_err(status)?;
        }
        Ok(deleted)
    }

    /// Spawn the background maintenance loop: on each `config.interval` tick it
    /// reaps expired leases and runs rolling-window compaction. Returns the
    /// task handle (abort it to stop the loop). Errors on a tick are swallowed
    /// (the next tick retries) — a transient backend hiccup must not kill the
    /// datastore's housekeeping.
    #[must_use]
    pub fn spawn_compactor(&self, config: CompactionConfig) -> tokio::task::JoinHandle<()> {
        let server = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(config.interval).await;
                let _ = server.reap_expired_leases().await;
                let _ = server.compact_retaining(config.retain).await;
            }
        })
    }
}

#[tonic::async_trait]
impl Maintenance for KineServer {
    async fn status(&self, _request: Request<StatusRequest>) -> Result<Response<StatusResponse>, Status> {
        let (header, db_size) = {
            let store = self.store.lock().await;
            let rev = store.current_revision().map_err(status)?;
            let size = store.db_size().map_err(status)?;
            (ResponseHeader { cluster_id: 0, member_id: 0, revision: rev, raft_term: 0 }, size)
        };
        Ok(Response::new(StatusResponse {
            header: Some(header),
            version: ETCD_VERSION.to_string(),
            db_size,
            leader: 0,
            raft_index: 0,
            raft_term: 0,
            raft_applied_index: 0,
            errors: Vec::new(),
            db_size_in_use: db_size,
            is_learner: false,
        }))
    }

    async fn defragment(
        &self,
        _request: Request<etcdserverpb::DefragmentRequest>,
    ) -> Result<Response<etcdserverpb::DefragmentResponse>, Status> {
        let reclaimed = {
            let store = self.store.lock().await;
            store.defragment().map_err(status)?
        };
        self.metrics.record_defragment(u64::try_from(reclaimed).unwrap_or(0));
        Ok(Response::new(etcdserverpb::DefragmentResponse { header: Some(self.header().await?) }))
    }
}

impl KineServer {
    /// Grant (or renew) a lease for `ttl` seconds. A request id of `0` makes the
    /// server allocate one; a non-zero id is honoured verbatim (etcd's
    /// `LeaseGrant`). Rejects a non-positive TTL.
    async fn do_lease_grant(&self, ttl: i64, id: i64) -> Result<LeaseGrantResponse, Status> {
        let lease_id = if id == 0 { self.next_lease_id.fetch_add(1, Ordering::Relaxed) } else { id };
        let now = (self.clock)();
        self.leases.lock().await.grant(lease_id, ttl, now).map_err(status)?;
        Ok(LeaseGrantResponse {
            header: Some(self.header().await?),
            id: lease_id,
            ttl,
            error: String::new(),
        })
    }

    /// Revoke a lease and delete every key attached to it (etcd `LeaseRevoke`).
    /// Unknown lease ids are a `NotFound`, matching etcd.
    async fn do_lease_revoke(&self, id: i64) -> Result<LeaseRevokeResponse, Status> {
        let existed = self.leases.lock().await.revoke(id).is_some();
        if !existed {
            return Err(Status::new(Code::NotFound, "etcdserver: requested lease not found"));
        }
        self.store.lock().await.revoke_lease_keys(id).map_err(status)?;
        Ok(LeaseRevokeResponse { header: Some(self.header().await?) })
    }

    /// Refresh one lease's TTL (etcd `LeaseKeepAlive`). A live lease is renewed
    /// from "now" and its TTL echoed back; a missing lease yields TTL `0` (the
    /// signal etcd sends a client to stop renewing) without erroring the stream.
    async fn do_keep_alive(&self, id: i64) -> Result<LeaseKeepAliveResponse, Status> {
        let now = (self.clock)();
        let ttl = {
            let mut leases = self.leases.lock().await;
            match leases.get(id).map(|l| l.ttl_seconds) {
                Some(ttl) => {
                    leases.grant(id, ttl, now).map_err(status)?;
                    ttl
                }
                None => 0,
            }
        };
        Ok(LeaseKeepAliveResponse { header: Some(self.header().await?), id, ttl })
    }

    /// Report a lease's remaining and granted TTL, and (optionally) the keys it
    /// owns (etcd `LeaseTimeToLive`). A missing lease reports TTL `-1`.
    async fn do_lease_time_to_live(
        &self,
        id: i64,
        want_keys: bool,
    ) -> Result<LeaseTimeToLiveResponse, Status> {
        let now = (self.clock)();
        let found = self.leases.lock().await.get(id).copied();
        let (ttl, granted_ttl) =
            found.map_or((-1, 0), |l| ((l.expires_at() - now).max(0), l.ttl_seconds));
        let keys = if want_keys && found.is_some() {
            self.store.lock().await.keys_with_lease(id).map_err(status)?
        } else {
            Vec::new()
        };
        Ok(LeaseTimeToLiveResponse { header: Some(self.header().await?), id, ttl, granted_ttl, keys })
    }
}

#[tonic::async_trait]
impl Lease for KineServer {
    async fn lease_grant(
        &self,
        request: Request<LeaseGrantRequest>,
    ) -> Result<Response<LeaseGrantResponse>, Status> {
        let req = request.into_inner();
        self.do_lease_grant(req.ttl, req.id).await.map(Response::new)
    }

    async fn lease_revoke(
        &self,
        request: Request<LeaseRevokeRequest>,
    ) -> Result<Response<LeaseRevokeResponse>, Status> {
        self.do_lease_revoke(request.into_inner().id).await.map(Response::new)
    }

    type LeaseKeepAliveStream =
        Pin<Box<dyn Stream<Item = Result<LeaseKeepAliveResponse, Status>> + Send>>;

    async fn lease_keep_alive(
        &self,
        request: Request<Streaming<LeaseKeepAliveRequest>>,
    ) -> Result<Response<Self::LeaseKeepAliveStream>, Status> {
        let mut inbound = request.into_inner();
        let server = self.clone();
        let stream = async_stream::try_stream! {
            while let Some(msg) = inbound.message().await? {
                yield server.do_keep_alive(msg.id).await?;
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn lease_time_to_live(
        &self,
        request: Request<LeaseTimeToLiveRequest>,
    ) -> Result<Response<LeaseTimeToLiveResponse>, Status> {
        let req = request.into_inner();
        self.do_lease_time_to_live(req.id, req.keys).await.map(Response::new)
    }
}

#[tonic::async_trait]
impl Watch for KineServer {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<WatchResponse, Status>> + Send>>;

    async fn watch(
        &self,
        request: Request<Streaming<WatchRequest>>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let mut inbound = request.into_inner();
        // Drive the stream from the first create request the client sends
        // (cancels / others before a create are ignored — the apiserver opens
        // one watch per stream).
        let create = loop {
            match inbound.message().await? {
                Some(WatchRequest {
                    request_union: Some(watch_request::RequestUnion::CreateRequest(c)),
                }) => break c,
                Some(_) => {}
                None => {
                    let empty: Self::WatchStream = Box::pin(tokio_stream::empty());
                    return Ok(Response::new(empty));
                }
            }
        };
        // Hand the remaining client stream to the watch so it can service
        // cancel requests (the create has already been consumed above).
        let stream: Self::WatchStream = Box::pin(self.watch_stream_with(create, inbound));
        Ok(Response::new(stream))
    }
}

/// Evaluate one etcd `Compare` against the current state of its key.
fn eval_compare(store: &SqliteStore, cmp: &Compare) -> Result<bool, KineError> {
    use etcdserverpb::compare::{CompareResult, CompareTarget, TargetUnion};

    let current = store.range(&KineRange::key(&cmp.key))?.kvs.into_iter().next();
    let (create_rev, mod_rev, version, value, lease) = current.as_ref().map_or(
        (0_i64, 0_i64, 0_i64, Vec::new(), 0_i64),
        |r| {
            (
                r.create_revision,
                r.mod_revision,
                r.mod_revision - r.create_revision + 1,
                r.value.clone(),
                r.lease,
            )
        },
    );

    let result = CompareResult::try_from(cmp.result).unwrap_or(CompareResult::Equal);
    let target = CompareTarget::try_from(cmp.target).unwrap_or(CompareTarget::Create);

    // Compare the requested target field against the stored one.
    let ordering = match (target, &cmp.target_union) {
        (CompareTarget::Create, Some(TargetUnion::CreateRevision(v))) => create_rev.cmp(v),
        (CompareTarget::Mod, Some(TargetUnion::ModRevision(v))) => mod_rev.cmp(v),
        (CompareTarget::Version, Some(TargetUnion::Version(v))) => version.cmp(v),
        (CompareTarget::Lease, Some(TargetUnion::Lease(v))) => lease.cmp(v),
        (CompareTarget::Value, Some(TargetUnion::Value(v))) => value.cmp(v),
        // A target with no matching union value compares as "equal to zero/empty".
        _ => std::cmp::Ordering::Equal,
    };

    Ok(match result {
        CompareResult::Equal => ordering.is_eq(),
        CompareResult::Greater => ordering.is_gt(),
        CompareResult::Less => ordering.is_lt(),
        CompareResult::NotEqual => ordering.is_ne(),
    })
}

/// Convert a kine [`Row`] into an etcd `KeyValue`. `version` is approximated as
/// `mod - create + 1` (kine does not store etcd's per-generation write counter;
/// the apiserver keys off `mod_revision`, which is exact).
fn row_to_kv(row: &Row) -> KeyValue {
    KeyValue {
        key: row.key.clone(),
        create_revision: row.create_revision,
        mod_revision: row.mod_revision,
        version: row.mod_revision - row.create_revision + 1,
        value: row.value.clone(),
        lease: row.lease,
    }
}

/// Convert a kine [`WatchEvent`] into an etcd `Event` (PUT / DELETE + the kv).
/// When `want_prev` is set, attach the `prev_kv` (the value the key held before
/// this change) — present for updates and deletes, absent for a fresh create
/// (which had no previous generation).
fn to_event(e: &WatchEvent, want_prev: bool) -> Event {
    use etcdserverpb::event::EventType;
    let kv = KeyValue {
        key: e.key.clone(),
        create_revision: e.create_revision,
        mod_revision: e.revision,
        version: e.revision - e.create_revision + 1,
        value: e.value.clone(),
        lease: 0,
    };
    let kind = match e.kind {
        EventKind::Put => EventType::Put,
        EventKind::Delete => EventType::Delete,
    };
    // A create is identified by mod == create; it has no previous kv.
    let has_prev = e.kind == EventKind::Delete || e.create_revision != e.revision;
    let prev_kv = (want_prev && has_prev).then(|| KeyValue {
        key: e.key.clone(),
        create_revision: e.create_revision,
        mod_revision: e.revision - 1,
        version: 0,
        value: e.prev_value.clone(),
        lease: 0,
    });
    Event { r#type: kind as i32, kv: Some(kv), prev_kv }
}

/// A `WatchResponse` marking the watch as canceled (the final message etcd
/// sends in response to a `WatchCancelRequest`).
fn watch_canceled(watch_id: i64, revision: i64) -> WatchResponse {
    WatchResponse {
        header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
        watch_id,
        created: false,
        canceled: true,
        compact_revision: 0,
        cancel_reason: "watch canceled".to_string(),
        fragment: false,
        events: Vec::new(),
    }
}

/// Build a `WatchResponse` carrying a header, watch id and event batch.
const fn watch_response(watch_id: i64, revision: i64, created: bool, events: Vec<Event>) -> WatchResponse {
    WatchResponse {
        header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
        watch_id,
        created,
        canceled: false,
        compact_revision: 0,
        cancel_reason: String::new(),
        fragment: false,
        events,
    }
}

/// Shape a backend [`KineRangeResp`] into the etcd `RangeResponse`, honouring
/// `count_only` / `keys_only`.
fn shape_range(req: &RangeRequest, resp: &KineRangeResp, revision: i64) -> RangeResponse {
    let kvs = if req.count_only {
        Vec::new()
    } else {
        resp.kvs
            .iter()
            .map(|r| {
                let mut kv = row_to_kv(r);
                if req.keys_only {
                    kv.value = Vec::new();
                }
                kv
            })
            .collect()
    };
    RangeResponse {
        header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
        kvs,
        more: resp.more,
        count: resp.count,
    }
}

/// Translate an etcd `RangeRequest` into a kine [`KineRange`].
fn to_kine_range(req: &RangeRequest) -> Result<KineRange, Status> {
    let mut k = to_kine_range_bytes(&req.key, &req.range_end)?;
    if req.revision < 0 {
        return Err(Status::new(Code::OutOfRange, "etcdserver: mvcc: revision is negative"));
    }
    k.revision = req.revision;
    if req.limit < 0 {
        return Err(Status::new(Code::InvalidArgument, "etcdserver: limit is negative"));
    }
    k.limit = req.limit;
    Ok(k)
}

/// Translate `(key, range_end)` into a kine range selector, applying etcd's
/// conventions: empty `range_end` → point get; `key="\0", range_end="\0"` →
/// whole keyspace; `range_end == key+1` → prefix; otherwise the explicit
/// half-open interval `[key, range_end)` (covers paginated list continuations).
fn to_kine_range_bytes(key: &[u8], range_end: &[u8]) -> Result<KineRange, Status> {
    if key.is_empty() && range_end.is_empty() {
        return Err(Status::new(Code::InvalidArgument, "etcdserver: key is not provided"));
    }
    let end = if range_end.is_empty() {
        RangeEnd::Single
    } else if key == [0].as_slice() && range_end == [0].as_slice() {
        RangeEnd::AllKeys
    } else if range_end == prefix_successor(key).as_slice() {
        RangeEnd::Prefix
    } else {
        RangeEnd::Explicit(range_end.to_vec())
    };
    Ok(KineRange { key: key.to_vec(), end, revision: 0, limit: 0 })
}

/// Map a kine error onto an etcd gRPC status, preserving etcd's well-known
/// messages so clients react correctly (notably the compacted-revision guard).
/// Takes the error by value so it slots into `Result::map_err`.
#[allow(clippy::needless_pass_by_value)]
fn status(err: KineError) -> Status {
    let code = match err {
        KineError::Compacted { .. } | KineError::FutureRevision { .. } => Code::OutOfRange,
        KineError::EmptyKey
        | KineError::InvalidRange
        | KineError::NegativeLimit { .. }
        | KineError::NegativeRevision { .. }
        | KineError::InvalidLeaseId
        | KineError::InvalidTtl { .. }
        | KineError::CompactionNotForward { .. }
        | KineError::CompactFutureRevision { .. } => Code::InvalidArgument,
        KineError::Backend { .. } => Code::Internal,
    };
    Status::new(code, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::etcdserverpb::{
        compare::{CompareResult, CompareTarget, TargetUnion},
        event::EventType,
        kv_server::Kv,
        maintenance_server::Maintenance,
        request_op, CompactionRequest, Compare, DeleteRangeRequest, PutRequest, RangeRequest,
        RequestOp, StatusRequest, TxnRequest, WatchCreateRequest,
    };
    use super::*;
    use crate::range::prefix_successor;

    fn server() -> KineServer {
        KineServer::new(SqliteStore::open_in_memory().unwrap())
    }

    fn put_req(key: &[u8], value: &[u8]) -> PutRequest {
        PutRequest {
            key: key.to_vec(),
            value: value.to_vec(),
            lease: 0,
            prev_kv: false,
            ignore_value: false,
            ignore_lease: false,
        }
    }

    fn point(key: &[u8]) -> RangeRequest {
        RangeRequest { key: key.to_vec(), ..Default::default() }
    }

    fn prefix(p: &[u8]) -> RangeRequest {
        RangeRequest { key: p.to_vec(), range_end: prefix_successor(p), ..Default::default() }
    }

    #[tokio::test]
    async fn put_then_range_round_trips_value_and_revision() {
        let s = server();
        let put = s.put(Request::new(put_req(b"/k", b"v1"))).await.unwrap().into_inner();
        assert_eq!(put.header.unwrap().revision, 1);

        let resp = s.range(Request::new(point(b"/k"))).await.unwrap().into_inner();
        assert_eq!(resp.kvs.len(), 1);
        assert_eq!(resp.kvs[0].value, b"v1");
        assert_eq!(resp.kvs[0].mod_revision, 1);
        assert_eq!(resp.kvs[0].create_revision, 1);
        assert_eq!(resp.header.unwrap().revision, 1);
        assert_eq!(resp.count, 1);
    }

    #[tokio::test]
    async fn range_prefix_returns_subtree_sorted_with_count() {
        let s = server();
        s.put(Request::new(put_req(b"/reg/a", b"1"))).await.unwrap();
        s.put(Request::new(put_req(b"/reg/b", b"2"))).await.unwrap();
        s.put(Request::new(put_req(b"/other", b"9"))).await.unwrap();
        let resp = s.range(Request::new(prefix(b"/reg/"))).await.unwrap().into_inner();
        let keys: Vec<_> = resp.kvs.iter().map(|kv| kv.key.clone()).collect();
        assert_eq!(keys, vec![b"/reg/a".to_vec(), b"/reg/b".to_vec()]);
        assert_eq!(resp.count, 2);
    }

    #[tokio::test]
    async fn put_with_prev_kv_returns_the_replaced_value() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v1"))).await.unwrap();
        let mut req = put_req(b"/k", b"v2");
        req.prev_kv = true;
        let resp = s.put(Request::new(req)).await.unwrap().into_inner();
        assert_eq!(resp.prev_kv.unwrap().value, b"v1");
    }

    #[tokio::test]
    async fn put_ignore_value_keeps_existing_value_updates_lease() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"keep"))).await.unwrap();
        let mut req = put_req(b"/k", b"");
        req.ignore_value = true;
        req.lease = 42;
        s.put(Request::new(req)).await.unwrap();
        let resp = s.range(Request::new(point(b"/k"))).await.unwrap().into_inner();
        assert_eq!(resp.kvs[0].value, b"keep");
        assert_eq!(resp.kvs[0].lease, 42);
    }

    #[tokio::test]
    async fn delete_range_tombstones_and_counts() {
        let s = server();
        s.put(Request::new(put_req(b"/reg/a", b"1"))).await.unwrap();
        s.put(Request::new(put_req(b"/reg/b", b"2"))).await.unwrap();
        let del = DeleteRangeRequest {
            key: b"/reg/".to_vec(),
            range_end: prefix_successor(b"/reg/"),
            prev_kv: true,
        };
        let resp = s.delete_range(Request::new(del)).await.unwrap().into_inner();
        assert_eq!(resp.deleted, 2);
        assert_eq!(resp.prev_kvs.len(), 2);
        assert_eq!(s.range(Request::new(prefix(b"/reg/"))).await.unwrap().into_inner().count, 0);
    }

    #[tokio::test]
    async fn range_at_compacted_revision_is_out_of_range() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v1"))).await.unwrap(); // 1
        s.put(Request::new(put_req(b"/k", b"v2"))).await.unwrap(); // 2
        s.put(Request::new(put_req(b"/k", b"v3"))).await.unwrap(); // 3
        s.compact(Request::new(CompactionRequest { revision: 2, physical: true }))
            .await
            .unwrap();
        let mut at1 = point(b"/k");
        at1.revision = 1;
        let err = s.range(Request::new(at1)).await.unwrap_err();
        assert_eq!(err.code(), Code::OutOfRange);
        assert!(err.message().contains("compacted"));
    }

    #[tokio::test]
    async fn txn_create_if_absent_runs_success_branch() {
        let s = server();
        // etcd's create idiom: compare create_revision == 0 (key absent).
        let txn = TxnRequest {
            compare: vec![Compare {
                result: CompareResult::Equal as i32,
                target: CompareTarget::Create as i32,
                key: b"/k".to_vec(),
                target_union: Some(TargetUnion::CreateRevision(0)),
                range_end: Vec::new(),
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(put_req(b"/k", b"created"))),
            }],
            failure: Vec::new(),
        };
        let resp = s.txn(Request::new(txn)).await.unwrap().into_inner();
        assert!(resp.succeeded);
        assert_eq!(s.range(Request::new(point(b"/k"))).await.unwrap().into_inner().kvs[0].value, b"created");
    }

    #[tokio::test]
    async fn txn_create_if_absent_takes_failure_branch_when_present() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"already"))).await.unwrap();
        let txn = TxnRequest {
            compare: vec![Compare {
                result: CompareResult::Equal as i32,
                target: CompareTarget::Create as i32,
                key: b"/k".to_vec(),
                target_union: Some(TargetUnion::CreateRevision(0)),
                range_end: Vec::new(),
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(put_req(b"/k", b"new"))),
            }],
            failure: vec![RequestOp {
                request: Some(request_op::Request::RequestRange(point(b"/k"))),
            }],
        };
        let resp = s.txn(Request::new(txn)).await.unwrap().into_inner();
        assert!(!resp.succeeded);
        // failure branch ran a range, value unchanged
        assert_eq!(s.range(Request::new(point(b"/k"))).await.unwrap().into_inner().kvs[0].value, b"already");
    }

    #[tokio::test]
    async fn txn_compare_and_swap_on_mod_revision() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v1"))).await.unwrap(); // mod 1
        // CAS: if mod_revision == 1 then put v2.
        let txn = TxnRequest {
            compare: vec![Compare {
                result: CompareResult::Equal as i32,
                target: CompareTarget::Mod as i32,
                key: b"/k".to_vec(),
                target_union: Some(TargetUnion::ModRevision(1)),
                range_end: Vec::new(),
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(put_req(b"/k", b"v2"))),
            }],
            failure: Vec::new(),
        };
        let resp = s.txn(Request::new(txn)).await.unwrap().into_inner();
        assert!(resp.succeeded);
        assert_eq!(s.range(Request::new(point(b"/k"))).await.unwrap().into_inner().kvs[0].value, b"v2");
    }

    #[tokio::test]
    async fn count_only_returns_count_without_kvs() {
        let s = server();
        s.put(Request::new(put_req(b"/reg/a", b"1"))).await.unwrap();
        s.put(Request::new(put_req(b"/reg/b", b"2"))).await.unwrap();
        let mut req = prefix(b"/reg/");
        req.count_only = true;
        let resp = s.range(Request::new(req)).await.unwrap().into_inner();
        assert_eq!(resp.count, 2);
        assert!(resp.kvs.is_empty());
    }

    #[tokio::test]
    async fn keys_only_strips_values() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"secret"))).await.unwrap();
        let mut req = point(b"/k");
        req.keys_only = true;
        let resp = s.range(Request::new(req)).await.unwrap().into_inner();
        assert_eq!(resp.kvs[0].key, b"/k");
        assert!(resp.kvs[0].value.is_empty());
    }

    #[tokio::test]
    async fn empty_key_put_is_invalid_argument() {
        let s = server();
        let err = s.put(Request::new(put_req(b"", b"v"))).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn maintenance_status_reports_etcd_version_and_header() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v"))).await.unwrap();
        let resp = s.status(Request::new(StatusRequest {})).await.unwrap().into_inner();
        assert_eq!(resp.version, ETCD_VERSION);
        assert_eq!(resp.header.unwrap().revision, 1);
    }

    #[tokio::test]
    async fn handlers_record_live_metrics() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v"))).await.unwrap(); // ok put
        s.put(Request::new(put_req(b"", b"x"))).await.unwrap_err(); // failed put (empty key)
        s.range(Request::new(point(b"/k"))).await.unwrap(); // ok range
        s.compact(Request::new(CompactionRequest { revision: 1, physical: true }))
            .await
            .unwrap();

        let out = s.metrics().render();
        assert!(out.contains("kine_request_total{operation=\"put\"} 2"));
        assert!(out.contains("kine_request_errors_total{operation=\"put\"} 1"));
        assert!(out.contains("kine_request_total{operation=\"range\"} 1"));
        assert!(out.contains("kine_compaction_runs_total 1"));
    }

    /// A server whose lease clock is the shared `now` cell — lets a test drive
    /// lease expiry / keep-alive renewal deterministically without sleeping.
    fn server_with_clock() -> (KineServer, Arc<std::sync::atomic::AtomicI64>) {
        let now = Arc::new(std::sync::atomic::AtomicI64::new(0));
        let n = Arc::clone(&now);
        let clock: super::ClockFn =
            Arc::new(move || n.load(std::sync::atomic::Ordering::Relaxed));
        (KineServer::with_clock(SqliteStore::open_in_memory().unwrap(), clock), now)
    }

    #[tokio::test]
    async fn lease_grant_allocates_an_id_when_zero_and_echoes_ttl() {
        let s = server();
        let resp = s.do_lease_grant(30, 0).await.unwrap();
        assert_ne!(resp.id, 0, "server allocates a non-zero lease id");
        assert_eq!(resp.ttl, 30);
        assert!(resp.error.is_empty());
    }

    #[tokio::test]
    async fn lease_grant_honours_an_explicit_id() {
        let s = server();
        let resp = s.do_lease_grant(30, 42).await.unwrap();
        assert_eq!(resp.id, 42);
    }

    #[tokio::test]
    async fn lease_grant_rejects_non_positive_ttl() {
        let s = server();
        let err = s.do_lease_grant(0, 0).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn lease_revoke_deletes_every_key_attached_to_the_lease() {
        let s = server();
        s.do_lease_grant(100, 5).await.unwrap();
        let mut p = put_req(b"/k1", b"v1");
        p.lease = 5;
        s.put(Request::new(p)).await.unwrap();
        let mut p2 = put_req(b"/k2", b"v2");
        p2.lease = 5;
        s.put(Request::new(p2)).await.unwrap();
        s.put(Request::new(put_req(b"/free", b"x"))).await.unwrap(); // no lease

        s.do_lease_revoke(5).await.unwrap();
        assert!(s.range(Request::new(point(b"/k1"))).await.unwrap().into_inner().kvs.is_empty());
        assert!(s.range(Request::new(point(b"/k2"))).await.unwrap().into_inner().kvs.is_empty());
        // the unleased key is untouched
        assert_eq!(
            s.range(Request::new(point(b"/free"))).await.unwrap().into_inner().kvs[0].value,
            b"x"
        );
    }

    #[tokio::test]
    async fn lease_revoke_of_unknown_lease_is_not_found() {
        let s = server();
        let err = s.do_lease_revoke(999).await.unwrap_err();
        assert_eq!(err.code(), Code::NotFound);
    }

    #[tokio::test]
    async fn lease_time_to_live_reports_remaining_granted_and_keys() {
        let (s, now) = server_with_clock();
        s.do_lease_grant(100, 7).await.unwrap(); // granted at now=0
        let mut p = put_req(b"/owned", b"v");
        p.lease = 7;
        s.put(Request::new(p)).await.unwrap();

        now.store(30, std::sync::atomic::Ordering::Relaxed);
        let resp = s.do_lease_time_to_live(7, true).await.unwrap();
        assert_eq!(resp.id, 7);
        assert_eq!(resp.granted_ttl, 100);
        assert_eq!(resp.ttl, 70, "remaining = granted - elapsed");
        assert_eq!(resp.keys, vec![b"/owned".to_vec()]);
    }

    #[tokio::test]
    async fn lease_time_to_live_of_unknown_lease_reports_negative_ttl() {
        let s = server();
        let resp = s.do_lease_time_to_live(404, false).await.unwrap();
        assert_eq!(resp.ttl, -1, "etcd signals a missing lease with TTL -1");
    }

    #[tokio::test]
    async fn lease_keep_alive_renews_the_ttl_and_postpones_expiry() {
        let (s, now) = server_with_clock();
        s.do_lease_grant(10, 3).await.unwrap(); // expires at 10
        now.store(8, std::sync::atomic::Ordering::Relaxed);
        let ka = s.do_keep_alive(3).await.unwrap();
        assert_eq!(ka.id, 3);
        assert_eq!(ka.ttl, 10);
        // renewed at 8 -> now expires at 18; at t=12 still alive with ~6s left
        now.store(12, std::sync::atomic::Ordering::Relaxed);
        let ttl = s.do_lease_time_to_live(3, false).await.unwrap();
        assert_eq!(ttl.ttl, 6);
    }

    #[tokio::test]
    async fn lease_keep_alive_of_missing_lease_returns_zero_ttl() {
        let s = server();
        let ka = s.do_keep_alive(123).await.unwrap();
        assert_eq!(ka.ttl, 0, "etcd keep-alive of a dead lease yields TTL 0");
    }

    #[tokio::test]
    async fn compact_retaining_drops_old_history_keeping_recent_revisions() {
        let s = server();
        for i in 0..6 {
            s.put(Request::new(put_req(b"/k", format!("v{i}").as_bytes()))).await.unwrap();
        } // revisions 1..=6, current value v5
        // retain the last 2 revisions -> compact floor moves to 4.
        let report = s.compact_retaining(2).await.unwrap().expect("compaction ran");
        assert_eq!(report.compacted, 4);
        // current state intact
        assert_eq!(s.range(Request::new(point(b"/k"))).await.unwrap().into_inner().kvs[0].value, b"v5");
        // a read below the new floor is now compacted-out
        let mut at1 = point(b"/k");
        at1.revision = 1;
        let err = s.range(Request::new(at1)).await.unwrap_err();
        assert_eq!(err.code(), Code::OutOfRange);
    }

    #[tokio::test]
    async fn compact_retaining_is_a_noop_when_nothing_is_old_enough() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v"))).await.unwrap(); // rev 1
        // retaining 1000 revisions on a 1-revision store: target <= 0 -> skip.
        assert!(s.compact_retaining(1000).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reap_expired_leases_deletes_their_keys_and_forgets_them() {
        let (s, now) = server_with_clock();
        s.do_lease_grant(10, 5).await.unwrap(); // expires at 10
        let mut p = put_req(b"/owned", b"v");
        p.lease = 5;
        s.put(Request::new(p)).await.unwrap();

        now.store(10, std::sync::atomic::Ordering::Relaxed); // TTL elapsed
        let deleted = s.reap_expired_leases().await.unwrap();
        assert_eq!(deleted, 1, "the expired lease's one key is reaped");
        assert!(s.range(Request::new(point(b"/owned"))).await.unwrap().into_inner().kvs.is_empty());
        // the lease is gone from the registry
        assert_eq!(s.do_lease_time_to_live(5, false).await.unwrap().ttl, -1);
    }

    #[tokio::test]
    async fn reap_leaves_unexpired_leases_alone() {
        let (s, now) = server_with_clock();
        s.do_lease_grant(100, 5).await.unwrap();
        let mut p = put_req(b"/owned", b"v");
        p.lease = 5;
        s.put(Request::new(p)).await.unwrap();
        now.store(50, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(s.reap_expired_leases().await.unwrap(), 0);
        assert_eq!(s.range(Request::new(point(b"/owned"))).await.unwrap().into_inner().kvs.len(), 1);
    }

    #[tokio::test]
    async fn maintenance_status_reports_a_real_db_size() {
        let s = server();
        s.put(Request::new(put_req(b"/k", b"v"))).await.unwrap();
        let resp = s.status(Request::new(StatusRequest {})).await.unwrap().into_inner();
        assert!(resp.db_size > 0, "status carries the real datastore size");
    }

    #[tokio::test]
    async fn defragment_rpc_rebuilds_and_returns_a_header() {
        let s = server();
        for i in 0..50 {
            s.put(Request::new(put_req(b"/k", format!("v{i}").as_bytes()))).await.unwrap();
        }
        s.compact(Request::new(CompactionRequest { revision: 40, physical: true })).await.unwrap();
        let resp = s
            .defragment(Request::new(super::etcdserverpb::DefragmentRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.header.is_some());
        // data still readable after the rebuild
        assert_eq!(s.range(Request::new(point(b"/k"))).await.unwrap().into_inner().kvs[0].value, b"v49");
    }

    fn watch_create(prefix_key: &[u8], start_revision: i64) -> WatchCreateRequest {
        WatchCreateRequest {
            key: prefix_key.to_vec(),
            range_end: prefix_successor(prefix_key),
            start_revision,
            progress_notify: false,
            filters: Vec::new(),
            prev_kv: false,
            watch_id: 7,
        }
    }

    #[tokio::test]
    async fn watch_stream_emits_created_then_replays_historical_events() {
        use tokio_stream::StreamExt;
        let s = server();
        s.put(Request::new(put_req(b"/ns/a", b"1"))).await.unwrap(); // rev 1 PUT
        s.put(Request::new(put_req(b"/ns/b", b"2"))).await.unwrap(); // rev 2 PUT
        let del = DeleteRangeRequest { key: b"/ns/a".to_vec(), range_end: Vec::new(), prev_kv: false };
        s.delete_range(Request::new(del)).await.unwrap(); //            rev 3 DELETE /ns/a

        let mut stream = Box::pin(s.watch_stream(watch_create(b"/ns/", 1)));

        let created = stream.next().await.unwrap().unwrap();
        assert!(created.created);
        assert_eq!(created.watch_id, 7);
        assert!(created.events.is_empty());

        let batch = stream.next().await.unwrap().unwrap();
        let revs: Vec<_> = batch.events.iter().map(|e| e.kv.as_ref().unwrap().mod_revision).collect();
        assert_eq!(revs, vec![1, 2, 3], "all changes since rev 1, in order");
        assert_eq!(batch.events[0].r#type, EventType::Put as i32);
        assert_eq!(batch.events[2].r#type, EventType::Delete as i32);
        assert_eq!(batch.events[2].kv.as_ref().unwrap().key, b"/ns/a");
    }

    #[tokio::test]
    async fn watch_stream_attaches_prev_kv_on_update_but_not_on_create() {
        use tokio_stream::StreamExt;
        let s = server();
        s.put(Request::new(put_req(b"/ns/k", b"v1"))).await.unwrap(); // rev 1 create
        s.put(Request::new(put_req(b"/ns/k", b"v2"))).await.unwrap(); // rev 2 update (old v1)

        let mut create = watch_create(b"/ns/", 1); // replay rev 1.. with prev_kv
        create.prev_kv = true;
        let mut stream = Box::pin(s.watch_stream(create));
        let _created = stream.next().await.unwrap().unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        let ev_create = batch.events.iter().find(|e| e.kv.as_ref().unwrap().mod_revision == 1).unwrap();
        assert!(ev_create.prev_kv.is_none(), "a create has no previous kv");
        let ev_update = batch.events.iter().find(|e| e.kv.as_ref().unwrap().mod_revision == 2).unwrap();
        assert_eq!(ev_update.prev_kv.as_ref().unwrap().value, b"v1");
    }

    #[tokio::test]
    async fn watch_stream_cancel_request_ends_with_a_canceled_response() {
        use tokio_stream::StreamExt;
        let s = server();
        s.put(Request::new(put_req(b"/ns/k", b"v"))).await.unwrap();
        let cancel = WatchRequest {
            request_union: Some(watch_request::RequestUnion::CancelRequest(
                super::etcdserverpb::WatchCancelRequest { watch_id: 7 },
            )),
        };
        let inbound = tokio_stream::iter(vec![Ok::<_, Status>(cancel)]);
        let mut stream = Box::pin(s.watch_stream_with(watch_create(b"/ns/", 1), inbound));

        let created = stream.next().await.unwrap().unwrap();
        assert!(created.created);
        // Drain until the cancellation marker arrives, then the stream ends.
        let canceled = loop {
            let r = stream.next().await.unwrap().unwrap();
            if r.canceled {
                break r;
            }
        };
        assert!(canceled.canceled);
        assert_eq!(canceled.watch_id, 7);
        assert!(stream.next().await.is_none(), "stream terminates after cancel");
    }

    #[tokio::test]
    async fn watch_stream_progress_notify_emits_an_empty_progress_response_when_idle() {
        use tokio_stream::StreamExt;
        let s = server(); // empty store: no events will ever fire
        let inbound = tokio_stream::iter(Vec::<Result<WatchRequest, Status>>::new());
        let mut create = watch_create(b"/ns/", 0); // from now; nothing happens
        create.progress_notify = true;
        let mut stream = Box::pin(s.watch_stream_with(create, inbound));

        let created = stream.next().await.unwrap().unwrap();
        assert!(created.created);
        // A progress notification: not created, not canceled, no events, carries
        // the current revision so the watcher can advance its checkpoint.
        let progress = stream.next().await.unwrap().unwrap();
        assert!(!progress.created);
        assert!(!progress.canceled);
        assert!(progress.events.is_empty());
        assert_eq!(progress.watch_id, 7);
    }

    #[tokio::test]
    async fn watch_stream_filters_to_its_prefix() {
        use tokio_stream::StreamExt;
        let s = server();
        s.put(Request::new(put_req(b"/ns/x", b"1"))).await.unwrap();
        s.put(Request::new(put_req(b"/other", b"2"))).await.unwrap();
        let mut stream = Box::pin(s.watch_stream(watch_create(b"/ns/", 1)));
        let _created = stream.next().await.unwrap().unwrap();
        let batch = stream.next().await.unwrap().unwrap();
        let keys: Vec<_> = batch.events.iter().map(|e| e.kv.as_ref().unwrap().key.clone()).collect();
        assert_eq!(keys, vec![b"/ns/x".to_vec()]);
    }
}
