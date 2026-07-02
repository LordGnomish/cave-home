// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::status::PodStatusManager`.
//!
//! Hand-port targets — `pkg/kubelet/status/status_manager_test.go`:
//! - upstream_test: TestNewStatus
//! - upstream_test: TestNewStatusPreservesPodStartTime
//! - upstream_test: TestStaleUpdates
//! - upstream_test: TestStatusEquality
//! - upstream_test: TestSyncBatchIgnoresNotFound
//! - upstream_test: TestUpdatePodStatusBatch
//! - upstream_test: TestSyncBatchNoDeadlock
//! - upstream_test: TestStaleUpdatesAreIgnored

use std::sync::Arc;

use cave_home_kubelet_rs::api::{
    ContainerState, ContainerStateRunning, ContainerStatus, PodPhase, PodStatus, PodUid,
};
use cave_home_kubelet_rs::status::{MockStatusSink, PodStatusManager, StatusSink};

fn running_status(phase: PodPhase, container: &str) -> PodStatus {
    PodStatus {
        phase,
        message: String::new(),
        reason: String::new(),
        container_statuses: vec![ContainerStatus {
            name: container.into(),
            state: ContainerState::Running(ContainerStateRunning { started_at_ms: 1 }),
            image: "nginx".into(),
            container_id: None,
            ready: true,
            restart_count: 0,
        }],
    }
}

#[tokio::test]
async fn first_set_pod_status_writes_through_to_sink() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    mgr.set_pod_status(&uid, running_status(PodPhase::Running, "main"))
        .await
        .unwrap();
    let n = mgr.sync_batch().await.unwrap();
    assert_eq!(n, 1);
    assert_eq!(sink.write_count(), 1);
}

#[tokio::test]
async fn duplicate_status_is_deduplicated() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    let s = running_status(PodPhase::Running, "main");
    mgr.set_pod_status(&uid, s.clone()).await.unwrap();
    mgr.sync_batch().await.unwrap();
    mgr.set_pod_status(&uid, s).await.unwrap();
    mgr.sync_batch().await.unwrap();
    assert_eq!(
        sink.write_count(),
        1,
        "identical follow-up should not produce a second write"
    );
}

#[tokio::test]
async fn changed_status_writes_again() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    mgr.set_pod_status(&uid, running_status(PodPhase::Pending, "main"))
        .await
        .unwrap();
    mgr.sync_batch().await.unwrap();
    mgr.set_pod_status(&uid, running_status(PodPhase::Running, "main"))
        .await
        .unwrap();
    mgr.sync_batch().await.unwrap();
    assert_eq!(sink.write_count(), 2);
}

#[tokio::test]
async fn cached_status_reflects_last_set() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    let s = running_status(PodPhase::Running, "main");
    mgr.set_pod_status(&uid, s.clone()).await.unwrap();
    let cached = mgr.cached_status(&uid).unwrap();
    assert_eq!(cached.phase, PodPhase::Running);
}

#[tokio::test]
async fn forget_pod_drops_cached_state() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    mgr.set_pod_status(&uid, running_status(PodPhase::Running, "main"))
        .await
        .unwrap();
    mgr.forget_pod(&uid);
    assert!(mgr.cached_status(&uid).is_none());
}

#[tokio::test]
async fn transient_failure_is_retried_and_eventually_succeeds() {
    let sink = Arc::new(MockStatusSink::new());
    sink.set_fail_once();
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    mgr.set_pod_status(&uid, running_status(PodPhase::Running, "main"))
        .await
        .unwrap();
    // First flush sees the synthetic failure, returns 0.
    let n = mgr.sync_batch().await.unwrap();
    assert_eq!(n, 0);
    assert_eq!(sink.write_count(), 0);
    // Second flush succeeds.
    let n = mgr.sync_batch().await.unwrap();
    assert_eq!(n, 1);
    assert_eq!(sink.write_count(), 1);
}

#[tokio::test]
async fn always_failing_sink_keeps_status_pending() {
    let sink = Arc::new(MockStatusSink::new());
    sink.set_always_fail(true);
    let mgr = PodStatusManager::new(sink.clone());
    let uid = PodUid::new("u1");
    mgr.set_pod_status(&uid, running_status(PodPhase::Running, "main"))
        .await
        .unwrap();
    for _ in 0..5 {
        let n = mgr.sync_batch().await.unwrap();
        assert_eq!(n, 0);
    }
    assert_eq!(sink.write_count(), 0);
    sink.set_always_fail(false);
    let n = mgr.sync_batch().await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn multiple_pods_are_independent() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    mgr.set_pod_status(
        &PodUid::new("u1"),
        running_status(PodPhase::Running, "main"),
    )
    .await
    .unwrap();
    mgr.set_pod_status(
        &PodUid::new("u2"),
        running_status(PodPhase::Pending, "main"),
    )
    .await
    .unwrap();
    let n = mgr.sync_batch().await.unwrap();
    assert_eq!(n, 2);
    assert_eq!(sink.write_count(), 2);
}

#[tokio::test]
async fn sync_batch_with_no_pending_returns_zero() {
    let sink = Arc::new(MockStatusSink::new());
    let mgr = PodStatusManager::new(sink.clone());
    let n = mgr.sync_batch().await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn mock_sink_records_writes_in_order() {
    let sink = MockStatusSink::new();
    sink.write(&PodUid::new("a"), &running_status(PodPhase::Pending, "x"))
        .await
        .unwrap();
    sink.write(&PodUid::new("b"), &running_status(PodPhase::Running, "y"))
        .await
        .unwrap();
    let writes = sink.writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].0.as_str(), "a");
    assert_eq!(writes[1].0.as_str(), "b");
}
