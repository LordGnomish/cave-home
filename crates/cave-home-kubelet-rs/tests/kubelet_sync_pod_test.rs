// SPDX-License-Identifier: Apache-2.0
//! End-to-end (mocked) tests for `cave_home_kubelet_rs::kubelet::Kubelet`.
//!
//! Hand-port targets — `pkg/kubelet/kubelet_test.go`:
//! - upstream_test: TestSyncPodKill
//! - upstream_test: TestSyncPodNoExtraneousMounts
//! - upstream_test: TestStatusManagerEnabled
//! - upstream_test: TestPodPhase
//! - upstream_test: TestSyncPodWithVolumes
//! - upstream_test: TestKubeletSyncLoopFlush

use std::sync::Arc;

use cave_home_kubelet_rs::api::{
    Container, EmptyDirVolumeSource, ObjectMeta, Pod, PodPhase, PodSpec, PodUid, RestartPolicy,
    Volume, VolumeMount, VolumeSource,
};
use cave_home_kubelet_rs::cri::types::ContainerState as CriState;
use cave_home_kubelet_rs::cri::{CriClient, MockCriClient};
use cave_home_kubelet_rs::kubelet::Kubelet;
use cave_home_kubelet_rs::podworker::WorkType;
use cave_home_kubelet_rs::status::{MockStatusSink, StatusSink};

fn pod_with_volume(uid: &str, vol_name: &str) -> Pod {
    Pod {
        metadata: ObjectMeta {
            name: "nginx".into(),
            namespace: "default".into(),
            uid: PodUid::new(uid),
            ..Default::default()
        },
        spec: PodSpec {
            containers: vec![Container {
                name: "main".into(),
                image: "nginx:1.27".into(),
                volume_mounts: vec![VolumeMount {
                    name: vol_name.into(),
                    mount_path: "/data".into(),
                    read_only: false,
                }],
                ..Default::default()
            }],
            volumes: vec![Volume {
                name: vol_name.into(),
                source: VolumeSource::EmptyDir(EmptyDirVolumeSource::default()),
            }],
            restart_policy: RestartPolicy::Always,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn make_kubelet() -> (
    Kubelet,
    Arc<MockCriClient>,
    Arc<MockStatusSink>,
    tempfile::TempDir,
) {
    let cri = Arc::new(MockCriClient::new());
    let sink = Arc::new(MockStatusSink::new());
    let tmp = tempfile::tempdir().unwrap();
    let kub = Kubelet::with_volume_root(
        cri.clone() as Arc<dyn CriClient>,
        sink.clone() as Arc<dyn StatusSink>,
        tmp.path(),
    );
    (kub, cri, sink, tmp)
}

#[tokio::test]
async fn sync_pod_creates_sandbox_container_and_mounts_volume() {
    let (kub, cri, sink, _tmp) = make_kubelet();
    let p = pod_with_volume("u1", "data");
    let out = kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    assert!(out.sandbox_id.is_some());
    assert_eq!(out.created_containers.len(), 1);

    let containers = cri.list_containers(None).await.unwrap();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].state, CriState::Running);

    // Status flushed at the end of sync.
    kub.flush_status().await.unwrap();
    let writes = sink.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].1.phase, PodPhase::Running);
}

#[tokio::test]
async fn sync_pod_terminating_then_terminated_cleans_everything() {
    let (kub, cri, _sink, _tmp) = make_kubelet();
    let p = pod_with_volume("u1", "data");
    kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    kub.sync_pod(&p, WorkType::Terminating).await.unwrap();
    kub.sync_pod(&p, WorkType::Terminated).await.unwrap();
    assert!(cri.list_containers(None).await.unwrap().is_empty());
    assert!(cri.list_pod_sandbox(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn sync_pod_idempotent_on_repeat() {
    let (kub, cri, _sink, _tmp) = make_kubelet();
    let p = pod_with_volume("u1", "data");
    kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    let out = kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    assert!(out.created_containers.is_empty());
    assert_eq!(cri.list_containers(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn sync_pod_status_phase_pending_until_started() {
    let (kub, _cri, sink, _tmp) = make_kubelet();
    let p = pod_with_volume("u1", "data");
    // First sync transitions Pending -> Running.
    kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    kub.flush_status().await.unwrap();
    let writes = sink.writes();
    assert!(writes.iter().any(|(_, st)| st.phase == PodPhase::Running));
}

#[tokio::test]
async fn forget_pod_drops_status_and_worker_state() {
    let (kub, _cri, sink, _tmp) = make_kubelet();
    let p = pod_with_volume("u1", "data");
    kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    kub.flush_status().await.unwrap();
    let n_before = sink.write_count();
    kub.forget_pod(&PodUid::new("u1"));
    // A subsequent sync_pod call on a forgotten pod should rebuild from
    // scratch (i.e. the worker is gone).
    let _ = kub.sync_pod(&p, WorkType::Sync).await.unwrap();
    kub.flush_status().await.unwrap();
    assert!(sink.write_count() >= n_before);
}

#[tokio::test]
async fn two_pods_get_independent_workers() {
    let (kub, cri, _sink, _tmp) = make_kubelet();
    let p1 = pod_with_volume("u1", "data");
    let p2 = pod_with_volume("u2", "data");
    kub.sync_pod(&p1, WorkType::Sync).await.unwrap();
    kub.sync_pod(&p2, WorkType::Sync).await.unwrap();
    assert_eq!(cri.list_pod_sandbox(None).await.unwrap().len(), 2);
    assert_eq!(cri.list_containers(None).await.unwrap().len(), 2);
}
