// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! End-to-end gRPC round-trip: boot the kine etcd server on a real TCP socket
//! and drive it with the generated etcd **client** over the wire — the same
//! path a Kubernetes apiserver takes. This proves the transport + the `SQLite`
//! backend work together across a real connection, not just at the trait level.
//!
//! Run with: `cargo test -p cave-home-kine-rs --features grpc`.

#![cfg(feature = "grpc")]

use cave_home_kine_rs::grpc::etcdserverpb::{
    kv_client::KvClient, maintenance_client::MaintenanceClient, watch_client::WatchClient, compare,
    request_op, Compare, DeleteRangeRequest, PutRequest, RangeRequest, RequestOp, StatusRequest,
    TxnRequest, WatchCreateRequest, WatchRequest,
};
use cave_home_kine_rs::grpc::KineServer;
use cave_home_kine_rs::range::prefix_successor;
use cave_home_kine_rs::sqlite::SqliteStore;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

/// Boot the kine server on an ephemeral port; return its `http://` URL.
async fn boot() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = KineServer::new(SqliteStore::open_in_memory().unwrap());
    let kv = server.kv();
    let watch = server.watch();
    let maint = server.maintenance();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(kv)
            .add_service(watch)
            .add_service(maint)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    format!("http://{addr}")
}

fn put(key: &[u8], value: &[u8]) -> PutRequest {
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
async fn apiserver_style_put_list_delete_flow_over_the_wire() {
    let url = boot().await;
    let mut kv = KvClient::connect(url).await.unwrap();

    // The apiserver writes registry objects...
    kv.put(put(b"/registry/pods/default/a", b"podA")).await.unwrap();
    kv.put(put(b"/registry/pods/default/b", b"podB")).await.unwrap();
    let put_c = kv.put(put(b"/registry/pods/default/c", b"podC")).await.unwrap().into_inner();
    assert_eq!(put_c.header.unwrap().revision, 3);

    // ...then LISTs the namespace by prefix (informer initial list).
    let list = kv.range(prefix(b"/registry/pods/default/")).await.unwrap().into_inner();
    assert_eq!(list.count, 3);
    let keys: Vec<_> = list.kvs.iter().map(|kv| kv.key.clone()).collect();
    assert_eq!(
        keys,
        vec![
            b"/registry/pods/default/a".to_vec(),
            b"/registry/pods/default/b".to_vec(),
            b"/registry/pods/default/c".to_vec(),
        ]
    );

    // A point GET returns the single object.
    let got = kv.range(point(b"/registry/pods/default/b")).await.unwrap().into_inner();
    assert_eq!(got.kvs[0].value, b"podB");

    // DELETE one object.
    let del = kv
        .delete_range(DeleteRangeRequest {
            key: b"/registry/pods/default/b".to_vec(),
            range_end: Vec::new(),
            prev_kv: true,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(del.deleted, 1);
    assert_eq!(del.prev_kvs[0].value, b"podB");

    // The namespace now lists two objects.
    assert_eq!(kv.range(prefix(b"/registry/pods/default/")).await.unwrap().into_inner().count, 2);
}

#[tokio::test]
async fn txn_guarded_create_over_the_wire() {
    let url = boot().await;
    let mut kv = KvClient::connect(url).await.unwrap();

    // etcd create idiom: Txn { if create_revision(key)==0 then Put }.
    let make_create = || TxnRequest {
        compare: vec![Compare {
            result: compare::CompareResult::Equal as i32,
            target: compare::CompareTarget::Create as i32,
            key: b"/registry/leases/x".to_vec(),
            target_union: Some(compare::TargetUnion::CreateRevision(0)),
            range_end: Vec::new(),
        }],
        success: vec![RequestOp {
            request: Some(request_op::Request::RequestPut(put(b"/registry/leases/x", b"held"))),
        }],
        failure: Vec::new(),
    };

    // First create succeeds.
    assert!(kv.txn(make_create()).await.unwrap().into_inner().succeeded);
    // Second create sees create_revision != 0 and fails the guard.
    assert!(!kv.txn(make_create()).await.unwrap().into_inner().succeeded);
    // Value is the first writer's.
    let got = kv.range(point(b"/registry/leases/x")).await.unwrap().into_inner();
    assert_eq!(got.kvs[0].value, b"held");
}

#[tokio::test]
async fn watch_stream_delivers_historical_events_over_the_wire() {
    use tokio_stream::StreamExt;
    let url = boot().await;
    let mut kv = KvClient::connect(url.clone()).await.unwrap();
    kv.put(put(b"/registry/cm/a", b"1")).await.unwrap(); // rev 1
    kv.put(put(b"/registry/cm/b", b"2")).await.unwrap(); // rev 2

    let mut watch = WatchClient::connect(url).await.unwrap();
    let create = WatchRequest {
        request_union: Some(cave_home_kine_rs::grpc::etcdserverpb::watch_request::RequestUnion::CreateRequest(
            WatchCreateRequest {
                key: b"/registry/cm/".to_vec(),
                range_end: prefix_successor(b"/registry/cm/"),
                start_revision: 1,
                progress_notify: false,
                filters: Vec::new(),
                prev_kv: false,
                watch_id: 1,
            },
        )),
    };
    let outbound = tokio_stream::once(create);
    let mut inbound = watch.watch(outbound).await.unwrap().into_inner();

    let created = inbound.next().await.unwrap().unwrap();
    assert!(created.created);
    let batch = inbound.next().await.unwrap().unwrap();
    let revs: Vec<_> = batch.events.iter().map(|e| e.kv.as_ref().unwrap().mod_revision).collect();
    assert_eq!(revs, vec![1, 2]);
}

#[tokio::test]
async fn maintenance_status_over_the_wire() {
    let url = boot().await;
    let mut maint = MaintenanceClient::connect(url).await.unwrap();
    let status = maint.status(StatusRequest {}).await.unwrap().into_inner();
    assert!(status.version.starts_with("3."));
}
