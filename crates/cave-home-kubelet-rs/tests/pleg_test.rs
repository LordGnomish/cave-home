// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::pleg::GenericPleg`.
//!
//! Hand-port targets — `pkg/kubelet/pleg/generic_test.go`:
//! - upstream_test: TestRelisting
//! - upstream_test: TestRelistingForRunningContainer
//! - upstream_test: TestEventChannelFull
//! - upstream_test: TestRelistWithReinspection
//! - upstream_test: TestRecordContainerEvent
//! - upstream_test: TestUpdateRunningPod
//! - upstream_test: TestRelistWithCachedPodRecord
//! - upstream_test: TestSetPodRecord

use std::sync::Arc;

use cave_home_kubelet_rs::cri::types::{
    ContainerConfig, ContainerMetadata, ImageSpec, PodSandboxConfig, PodSandboxMetadata,
};
use cave_home_kubelet_rs::cri::{CriClient, MockCriClient};
use cave_home_kubelet_rs::pleg::{
    Clock, GenericPleg, MockClock, PodLifecycleEventType, SystemClock,
};

fn sandbox(name: &str, uid: &str) -> PodSandboxConfig {
    PodSandboxConfig {
        metadata: PodSandboxMetadata {
            name: name.into(),
            uid: uid.into(),
            namespace: "default".into(),
            attempt: 0,
        },
        ..Default::default()
    }
}

fn ctr(name: &str) -> ContainerConfig {
    ContainerConfig {
        metadata: ContainerMetadata {
            name: name.into(),
            attempt: 0,
        },
        image: ImageSpec {
            image: "nginx:1.27".into(),
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn first_relist_emits_started_for_each_running_container() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let sb = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, ctr("c"), sandbox("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();

    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let mut rx = pleg.subscribe();
    let n = pleg.relist().await;
    assert_eq!(n, 1, "one ContainerStarted expected on first relist");
    let evt = rx.try_recv().expect("event delivered");
    assert_eq!(evt.event_type, PodLifecycleEventType::ContainerStarted);
    assert_eq!(evt.container_id, cid);
    assert_eq!(evt.id.as_str(), "u");
}

#[tokio::test]
async fn relist_no_changes_emits_zero_events() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let sb = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, ctr("c"), sandbox("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();

    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let _ = pleg.relist().await;
    let mut rx = pleg.subscribe();
    let n = pleg.relist().await;
    assert_eq!(n, 0, "second relist sees no change");
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn relist_emits_died_when_container_exits() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let sb = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, ctr("c"), sandbox("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let mut rx = pleg.subscribe();
    let _ = pleg.relist().await; // started
    let _ = rx.try_recv();
    cri.stop_container(&cid, 0).await.unwrap();
    let n = pleg.relist().await;
    assert_eq!(n, 1);
    let evt = rx.try_recv().unwrap();
    assert_eq!(evt.event_type, PodLifecycleEventType::ContainerDied);
}

#[tokio::test]
async fn relist_emits_removed_when_container_disappears() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let sb = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, ctr("c"), sandbox("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let mut rx = pleg.subscribe();
    let _ = pleg.relist().await;
    let _ = rx.try_recv();
    cri.stop_container(&cid, 0).await.unwrap();
    cri.remove_container(&cid).await.unwrap();
    let _n = pleg.relist().await;
    // We expect ContainerRemoved for the missing container.
    let mut saw_removed = false;
    while let Ok(evt) = rx.try_recv() {
        if evt.event_type == PodLifecycleEventType::ContainerRemoved && evt.container_id == cid {
            saw_removed = true;
        }
    }
    assert!(saw_removed, "expected ContainerRemoved");
}

#[tokio::test]
async fn relist_handles_two_pods_independently() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let s1 = cri.run_pod_sandbox(sandbox("p1", "u1")).await.unwrap();
    let s2 = cri.run_pod_sandbox(sandbox("p2", "u2")).await.unwrap();
    let c1 = cri
        .create_container(&s1, ctr("c1"), sandbox("p1", "u1"))
        .await
        .unwrap();
    let c2 = cri
        .create_container(&s2, ctr("c2"), sandbox("p2", "u2"))
        .await
        .unwrap();
    cri.start_container(&c1).await.unwrap();
    cri.start_container(&c2).await.unwrap();

    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let n = pleg.relist().await;
    assert_eq!(n, 2);
}

#[tokio::test]
async fn relist_records_clock_timestamp() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(424242));
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let _ = pleg.relist().await;
    assert_eq!(pleg.last_relist_ms(), 424242);
}

#[tokio::test]
async fn relist_handles_empty_runtime() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let n = pleg.relist().await;
    assert_eq!(n, 0);
}

#[tokio::test]
async fn relist_emits_pod_sync_when_sandbox_appears() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let mut rx = pleg.subscribe();
    let _ = pleg.relist().await;
    let _ = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let _ = pleg.relist().await;
    let mut saw_sync = false;
    while let Ok(evt) = rx.try_recv() {
        if evt.event_type == PodLifecycleEventType::PodSync && evt.id.as_str() == "u" {
            saw_sync = true;
        }
    }
    assert!(saw_sync, "expected PodSync for new sandbox");
}

#[tokio::test]
async fn relist_emits_pod_sync_when_sandbox_disappears() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let sb = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let mut rx = pleg.subscribe();
    let _ = pleg.relist().await;
    while rx.try_recv().is_ok() {}
    cri.stop_pod_sandbox(&sb).await.unwrap();
    cri.remove_pod_sandbox(&sb).await.unwrap();
    let _ = pleg.relist().await;
    let mut saw_sync = false;
    while let Ok(evt) = rx.try_recv() {
        if evt.event_type == PodLifecycleEventType::PodSync {
            saw_sync = true;
        }
    }
    assert!(saw_sync);
}

#[tokio::test]
async fn mock_clock_advance_increments_now() {
    let c = MockClock::new(100);
    assert_eq!(c.now_unix_millis(), 100);
    c.advance(50);
    assert_eq!(c.now_unix_millis(), 150);
    c.set(0);
    assert_eq!(c.now_unix_millis(), 0);
}

#[tokio::test]
async fn system_clock_returns_positive_value() {
    let c = SystemClock::new();
    assert!(c.now_unix_millis() > 0);
}

#[tokio::test]
async fn pleg_supports_multiple_subscribers() {
    let cri = Arc::new(MockCriClient::new());
    let clock = Arc::new(MockClock::new(0));
    let pleg = GenericPleg::new(cri.clone(), clock.clone());
    let mut rx1 = pleg.subscribe();
    let mut rx2 = pleg.subscribe();
    let sb = cri.run_pod_sandbox(sandbox("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, ctr("c"), sandbox("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();
    let _ = pleg.relist().await;
    assert!(rx1.try_recv().is_ok());
    assert!(rx2.try_recv().is_ok());
}
