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
    kv_client::KvClient, lease_client::LeaseClient, maintenance_client::MaintenanceClient,
    watch_client::WatchClient, compare, request_op, CompactionRequest, Compare, DefragmentRequest,
    DeleteRangeRequest, LeaseGrantRequest, LeaseRevokeRequest, LeaseTimeToLiveRequest, PutRequest,
    RangeRequest, RequestOp, StatusRequest, TxnRequest, WatchCancelRequest, WatchCreateRequest,
    WatchRequest,
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
    let lease = server.lease();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(kv)
            .add_service(watch)
            .add_service(maint)
            .add_service(lease)
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

#[tokio::test]
async fn lease_attached_key_is_deleted_on_revoke_over_the_wire() {
    // The control-plane idiom: the apiserver grants a lease, attaches an object
    // to it (e.g. a control-plane masterlease / event TTL), then revokes it.
    let url = boot().await;
    let mut lease = LeaseClient::connect(url.clone()).await.unwrap();
    let mut kv = KvClient::connect(url).await.unwrap();

    let grant = lease.lease_grant(LeaseGrantRequest { ttl: 60, id: 0 }).await.unwrap().into_inner();
    assert_ne!(grant.id, 0, "server allocated a lease id");
    assert_eq!(grant.ttl, 60);

    let mut p = put(b"/registry/masterleases/node-a", b"holder");
    p.lease = grant.id;
    kv.put(p).await.unwrap();
    assert_eq!(kv.range(point(b"/registry/masterleases/node-a")).await.unwrap().into_inner().count, 1);

    // TimeToLive reports the granted TTL and the keys the lease owns.
    let ttl = lease
        .lease_time_to_live(LeaseTimeToLiveRequest { id: grant.id, keys: true })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(ttl.granted_ttl, 60);
    assert!(ttl.keys.iter().any(|k| k == b"/registry/masterleases/node-a"));

    // Revoking the lease deletes its attached key in one shot.
    lease.lease_revoke(LeaseRevokeRequest { id: grant.id }).await.unwrap();
    assert_eq!(kv.range(point(b"/registry/masterleases/node-a")).await.unwrap().into_inner().count, 0);
}

#[tokio::test]
async fn watch_cancel_over_the_wire_terminates_the_stream() {
    use std::time::Duration;
    use tokio_stream::StreamExt;
    let url = boot().await;
    let mut kv = KvClient::connect(url.clone()).await.unwrap();
    kv.put(put(b"/registry/cm/a", b"1")).await.unwrap();

    let create = WatchRequest {
        request_union: Some(cave_home_kine_rs::grpc::etcdserverpb::watch_request::RequestUnion::CreateRequest(
            WatchCreateRequest {
                key: b"/registry/cm/".to_vec(),
                range_end: prefix_successor(b"/registry/cm/"),
                start_revision: 1,
                progress_notify: false,
                filters: Vec::new(),
                prev_kv: false,
                watch_id: 9,
            },
        )),
    };
    let cancel = WatchRequest {
        request_union: Some(cave_home_kine_rs::grpc::etcdserverpb::watch_request::RequestUnion::CancelRequest(
            WatchCancelRequest { watch_id: 9 },
        )),
    };
    // Send the create, then a cancel a moment later, on the same client stream.
    let outbound = async_stream::stream! {
        yield create;
        tokio::time::sleep(Duration::from_millis(50)).await;
        yield cancel;
    };

    let mut watch = WatchClient::connect(url).await.unwrap();
    let mut inbound = watch.watch(outbound).await.unwrap().into_inner();
    let created = inbound.next().await.unwrap().unwrap();
    assert!(created.created);

    // Drain until the server acknowledges the cancellation, then the stream ends.
    let mut canceled = false;
    while let Some(resp) = inbound.next().await {
        let resp = resp.unwrap();
        if resp.canceled {
            assert_eq!(resp.watch_id, 9);
            canceled = true;
            break;
        }
    }
    assert!(canceled, "server sent a canceled marker");
}

#[tokio::test]
async fn compact_then_defragment_over_the_wire_preserves_current_state() {
    let url = boot().await;
    let mut kv = KvClient::connect(url.clone()).await.unwrap();
    for i in 0..20 {
        kv.put(put(b"/registry/churn", format!("v{i}").as_bytes())).await.unwrap();
    }
    kv.compact(CompactionRequest { revision: 15, physical: true }).await.unwrap();

    let mut maint = MaintenanceClient::connect(url).await.unwrap();
    maint.defragment(DefragmentRequest {}).await.unwrap();

    // The live value survives the compaction + rebuild.
    let got = kv.range(point(b"/registry/churn")).await.unwrap().into_inner();
    assert_eq!(got.kvs[0].value, b"v19");
}
