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
