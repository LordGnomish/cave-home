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

use std::sync::Arc;

use tokio::sync::Mutex;
use tonic::{Code, Request, Response, Status};

use crate::error::KineError;
use crate::range::{prefix_successor, RangeEnd, RangeRequest as KineRange, RangeResponse as KineRangeResp};
use crate::sqlite::SqliteStore;
use crate::store::Row;

/// The generated etcd protobuf types and service stubs.
pub mod etcdserverpb {
    #![allow(clippy::all, clippy::pedantic, clippy::nursery, missing_docs)]
    tonic::include_proto!("etcdserverpb");
}

use etcdserverpb::{
    kv_server::{Kv, KvServer},
    maintenance_server::{Maintenance, MaintenanceServer},
    request_op, response_op, CompactionRequest, CompactionResponse, Compare, DeleteRangeRequest,
    DeleteRangeResponse, KeyValue, PutRequest, PutResponse, RangeRequest, RangeResponse,
    RequestOp, ResponseHeader, ResponseOp, StatusRequest, StatusResponse, TxnRequest, TxnResponse,
};

/// kine's reported etcd server version (the apiserver only checks it is a
/// 3.x etcd that supports the v3 API).
pub const ETCD_VERSION: &str = "3.5.13";

/// An etcd gRPC server backed by a real [`SqliteStore`]. Cheap to clone (the
/// store is shared behind an `Arc`), so the same instance can back several
/// service servers ([`Self::kv`], [`Self::maintenance`]).
#[derive(Clone)]
pub struct KineServer {
    store: Arc<Mutex<SqliteStore>>,
}

impl KineServer {
    /// Wrap an owned store.
    #[must_use]
    pub fn new(store: SqliteStore) -> Self {
        Self { store: Arc::new(Mutex::new(store)) }
    }

    /// Wrap an already-shared store (so a watcher / metrics layer can share it).
    #[must_use]
    pub const fn from_shared(store: Arc<Mutex<SqliteStore>>) -> Self {
        Self { store }
    }

    /// The shared store handle.
    #[must_use]
    pub fn store(&self) -> Arc<Mutex<SqliteStore>> {
        Arc::clone(&self.store)
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
        Ok(Response::new(self.do_range(request.get_ref()).await?))
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        Ok(Response::new(self.do_put(request.get_ref()).await?))
    }

    async fn delete_range(
        &self,
        request: Request<DeleteRangeRequest>,
    ) -> Result<Response<DeleteRangeResponse>, Status> {
        Ok(Response::new(self.do_delete_range(request.get_ref()).await?))
    }

    async fn txn(&self, request: Request<TxnRequest>) -> Result<Response<TxnResponse>, Status> {
        let txn = request.into_inner();
        // Evaluate every comparison against the current committed state; etcd
        // runs the success branch iff all compares hold, else the failure one.
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
        Ok(Response::new(TxnResponse {
            header: Some(self.header().await?),
            succeeded,
            responses,
        }))
    }

    async fn compact(
        &self,
        request: Request<CompactionRequest>,
    ) -> Result<Response<CompactionResponse>, Status> {
        let req = request.into_inner();
        let mut store = self.store.lock().await;
        store.compact(req.revision).map_err(status)?;
        let revision = store.current_revision().map_err(status)?;
        Ok(Response::new(CompactionResponse {
            header: Some(ResponseHeader { cluster_id: 0, member_id: 0, revision, raft_term: 0 }),
        }))
    }
}

impl KineServer {
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

#[tonic::async_trait]
impl Maintenance for KineServer {
    async fn status(&self, _request: Request<StatusRequest>) -> Result<Response<StatusResponse>, Status> {
        let header = self.header().await?;
        Ok(Response::new(StatusResponse {
            header: Some(header),
            version: ETCD_VERSION.to_string(),
            db_size: 0,
            leader: 0,
            raft_index: 0,
            raft_term: 0,
            raft_applied_index: 0,
            errors: Vec::new(),
            db_size_in_use: 0,
            is_learner: false,
        }))
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
        kv_server::Kv,
        maintenance_server::Maintenance,
        request_op, CompactionRequest, Compare, DeleteRangeRequest, PutRequest, RangeRequest,
        RequestOp, StatusRequest, TxnRequest,
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
}
