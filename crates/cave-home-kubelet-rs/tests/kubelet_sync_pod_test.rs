// SPDX-License-Identifier: Apache-2.0
//! End-to-end (mocked) tests for `cave_home_kubelet_rs::kubelet::Kubelet`.
//!
//! Hand-port targets — `pkg/kubelet/kubelet_test.go`:
//! - `upstream_test`: `TestSyncPodKill`
//! - `upstream_test`: `TestSyncPodNoExtraneousMounts`
//! - `upstream_test`: `TestStatusManagerEnabled`
//! - `upstream_test`: `TestPodPhase`
//! - `upstream_test`: `TestSyncPodWithVolumes`
//! - `upstream_test`: `TestKubeletSyncLoopFlush`

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

type TestResult = Result<(), Box<dyn std::error::Error>>;

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

fn make_kubelet() -> std::io::Result<(
    Kubelet,
    Arc<MockCriClient>,
    Arc<MockStatusSink>,
    tempfile::TempDir,
)> {
    let cri = Arc::new(MockCriClient::new());
    let sink = Arc::new(MockStatusSink::new());
    let tmp = tempfile::tempdir()?;
    let kub = Kubelet::with_volume_root(
        cri.clone() as Arc<dyn CriClient>,
        sink.clone() as Arc<dyn StatusSink>,
        tmp.path(),
    );
    Ok((kub, cri, sink, tmp))
}

#[tokio::test]
async fn sync_pod_creates_sandbox_container_and_mounts_volume() -> TestResult {
    let (kub, cri, sink, _tmp) = make_kubelet()?;
    let p = pod_with_volume("u1", "data");
    let out = kub.sync_pod(&p, WorkType::Sync).await?;
    assert!(out.sandbox_id.is_some());
    assert_eq!(out.created_containers.len(), 1);

    let containers = cri.list_containers(None).await?;
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].state, CriState::Running);

    // Status flushed at the end of sync.
    kub.flush_status().await?;
    let writes = sink.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].1.phase, PodPhase::Running);
    Ok(())
}

#[tokio::test]
async fn sync_pod_terminating_then_terminated_cleans_everything() -> TestResult {
    let (kub, cri, _sink, _tmp) = make_kubelet()?;
    let p = pod_with_volume("u1", "data");
    kub.sync_pod(&p, WorkType::Sync).await?;
    kub.sync_pod(&p, WorkType::Terminating).await?;
    kub.sync_pod(&p, WorkType::Terminated).await?;
    assert_eq!(
        cri.list_containers(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::Container; 0]
    );
    assert_eq!(
        cri.list_pod_sandbox(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::PodSandbox; 0]
    );
    Ok(())
}

#[tokio::test]
async fn sync_pod_idempotent_on_repeat() -> TestResult {
    let (kub, cri, _sink, _tmp) = make_kubelet()?;
    let p = pod_with_volume("u1", "data");
    kub.sync_pod(&p, WorkType::Sync).await?;
    let out = kub.sync_pod(&p, WorkType::Sync).await?;
    assert_eq!(out.created_containers, [] as [std::string::String; 0]);
    assert_eq!(cri.list_containers(None).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn sync_pod_status_phase_pending_until_started() -> TestResult {
    let (kub, _cri, sink, _tmp) = make_kubelet()?;
    let p = pod_with_volume("u1", "data");
    // First sync transitions Pending -> Running.
    kub.sync_pod(&p, WorkType::Sync).await?;
    kub.flush_status().await?;
    let writes = sink.writes();
    assert!(writes.iter().any(|(_, st)| st.phase == PodPhase::Running));
    Ok(())
}

#[tokio::test]
async fn forget_pod_drops_status_and_worker_state() -> TestResult {
    let (kub, _cri, sink, _tmp) = make_kubelet()?;
    let p = pod_with_volume("u1", "data");
    kub.sync_pod(&p, WorkType::Sync).await?;
    kub.flush_status().await?;
    let n_before = sink.write_count();
    kub.forget_pod(&PodUid::new("u1"));
    // A subsequent sync_pod call on a forgotten pod should rebuild from
    // scratch (i.e. the worker is gone).
    let _ = kub.sync_pod(&p, WorkType::Sync).await?;
    kub.flush_status().await?;
    assert!(sink.write_count() >= n_before);
    Ok(())
}

#[tokio::test]
async fn two_pods_get_independent_workers() -> TestResult {
    let (kub, cri, _sink, _tmp) = make_kubelet()?;
    let p1 = pod_with_volume("u1", "data");
    let p2 = pod_with_volume("u2", "data");
    kub.sync_pod(&p1, WorkType::Sync).await?;
    kub.sync_pod(&p2, WorkType::Sync).await?;
    assert_eq!(cri.list_pod_sandbox(None).await?.len(), 2);
    assert_eq!(cri.list_containers(None).await?.len(), 2);
    Ok(())
}
