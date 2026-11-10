// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::podworker::PodWorker`.
//!
//! Hand-port targets — `pkg/kubelet/pod_workers_test.go`:
//! - `upstream_test`: `TestUpdatePod`
//! - `upstream_test`: `TestUpdatePodForRuntimePod`
//! - `upstream_test`: `TestSyncKnownPods`
//! - `upstream_test`: `TestPodSyncIfTerminationGracePeriodChanged`
//! - `upstream_test`: `TestPodWorkerHandlesPodsExternalUpdate`
//! - `upstream_test`: `TestSyncKnownPodsWithRequeueDelay`
//! - `upstream_test`: `TestKillPodSandbox`
//! - `upstream_test`: `TestSyncPodKillSandbox`
//! - `upstream_test`: `TestSyncPodTerminating`
//! - `upstream_test`: `TestSyncPodTerminated`
//! - `upstream_test`: `TestComputePodActionsContainerLifecycle`
//! - `upstream_test`: `TestComputePodActionsRestartPolicy`
//! - `upstream_test`: `TestComputePodActionsContainersRemoved`
//! - `upstream_test`: `TestComputePodActionsImageChanged`
//! - `upstream_test`: `TestComputePodActionsCrashLoopOff`

use std::sync::Arc;

use cave_home_kubelet_rs::api::{Container, ObjectMeta, Pod, PodSpec, PodUid, RestartPolicy};
use cave_home_kubelet_rs::cri::types::{ContainerState as CriState, PodSandboxState};
use cave_home_kubelet_rs::cri::{CriClient, MockCriClient};
use cave_home_kubelet_rs::podworker::{PodWorker, PodWorkerState, WorkType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn pod(name: &str, uid: &str, containers: Vec<&str>) -> Pod {
    Pod {
        metadata: ObjectMeta {
            name: name.into(),
            namespace: "default".into(),
            uid: PodUid::new(uid),
            ..Default::default()
        },
        spec: PodSpec {
            containers: containers
                .into_iter()
                .map(|n| Container {
                    name: n.into(),
                    image: "nginx:1.27".into(),
                    ..Default::default()
                })
                .collect(),
            restart_policy: RestartPolicy::Always,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn sync_creates_sandbox_and_starts_containers() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("nginx", "u1", vec!["main"]);
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert!(out.sandbox_id.is_some());
    assert_eq!(out.created_containers.len(), 1);
    assert_eq!(out.started_containers.len(), 1);

    let sandboxes = cri.list_pod_sandbox(None).await?;
    assert_eq!(sandboxes.len(), 1);
    let containers = cri.list_containers(None).await?;
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].state, CriState::Running);
    Ok(())
}

#[tokio::test]
async fn second_sync_is_a_no_op_when_already_in_desired_state() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("nginx", "u1", vec!["main"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert_eq!(out.created_containers, [] as [std::string::String; 0]);
    assert_eq!(out.started_containers, [] as [std::string::String; 0]);
    assert_eq!(out.killed_containers, [] as [std::string::String; 0]);
    Ok(())
}

#[tokio::test]
async fn sync_creates_multiple_containers() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("api", "u1", vec!["app", "sidecar"]);
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert_eq!(out.created_containers.len(), 2);
    assert_eq!(out.started_containers.len(), 2);
    Ok(())
}

#[tokio::test]
async fn sync_adds_container_when_spec_grows() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p1 = pod("api", "u1", vec!["app"]);
    let _ = worker.sync(&p1, WorkType::Sync).await?;
    let p2 = pod("api", "u1", vec!["app", "sidecar"]);
    let out = worker.sync(&p2, WorkType::Sync).await?;
    assert_eq!(out.created_containers.len(), 1);
    assert_eq!(out.started_containers.len(), 1);
    let containers = cri.list_containers(None).await?;
    assert_eq!(containers.len(), 2);
    Ok(())
}

#[tokio::test]
async fn sync_kills_container_no_longer_in_spec() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p1 = pod("api", "u1", vec!["app", "sidecar"]);
    let _ = worker.sync(&p1, WorkType::Sync).await?;
    let p2 = pod("api", "u1", vec!["app"]);
    let out = worker.sync(&p2, WorkType::Sync).await?;
    assert_eq!(out.killed_containers.len(), 1);
    Ok(())
}

#[tokio::test]
async fn sync_restarts_exited_container_with_restart_always() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("api", "u1", vec!["app"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    // External event: container exits.
    let containers = cri.list_containers(None).await?;
    let cid = containers[0].id.clone();
    cri.stop_container(&cid, 0).await?;
    // Sync again.
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert_eq!(out.created_containers.len(), 1);
    let containers = cri.list_containers(None).await?;
    assert!(containers.iter().any(|c| c.state == CriState::Running));
    Ok(())
}

#[tokio::test]
async fn sync_does_not_restart_with_never_policy() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let mut p = pod("api", "u1", vec!["app"]);
    p.spec.restart_policy = RestartPolicy::Never;
    let _ = worker.sync(&p, WorkType::Sync).await?;
    let containers = cri.list_containers(None).await?;
    let cid = containers[0].id.clone();
    cri.stop_container(&cid, 0).await?;
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert_eq!(out.created_containers, [] as [std::string::String; 0]);
    Ok(())
}

#[tokio::test]
async fn terminating_stops_all_containers() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("api", "u1", vec!["app", "sidecar"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    let out = worker.sync(&p, WorkType::Terminating).await?;
    assert_eq!(out.killed_containers.len(), 2);
    let cs = cri.list_containers(None).await?;
    for c in cs {
        assert_eq!(c.state, CriState::Exited);
    }
    assert_eq!(worker.state(), PodWorkerState::Terminating);
    Ok(())
}

#[tokio::test]
async fn terminated_removes_containers_and_sandbox() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("api", "u1", vec!["app"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    let _ = worker.sync(&p, WorkType::Terminating).await?;
    let _ = worker.sync(&p, WorkType::Terminated).await?;
    assert_eq!(
        cri.list_containers(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::Container; 0]
    );
    assert_eq!(
        cri.list_pod_sandbox(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::PodSandbox; 0]
    );
    assert_eq!(worker.state(), PodWorkerState::Terminated);
    Ok(())
}

#[tokio::test]
async fn syncing_a_terminated_pod_is_idempotent() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("api", "u1", vec!["app"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    let _ = worker.sync(&p, WorkType::Terminating).await?;
    let _ = worker.sync(&p, WorkType::Terminated).await?;
    let out = worker.sync(&p, WorkType::Terminated).await?;
    assert_eq!(out.killed_containers, [] as [std::string::String; 0]);
    assert_eq!(out.created_containers, [] as [std::string::String; 0]);
    Ok(())
}

#[tokio::test]
async fn sync_after_external_sandbox_loss_recreates_sandbox() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("api", "u1", vec!["app"]);
    let out1 = worker.sync(&p, WorkType::Sync).await?;
    let sb1 = out1.sandbox_id.ok_or("no sandbox")?;
    cri.stop_pod_sandbox(&sb1).await?;
    cri.remove_pod_sandbox(&sb1).await?;
    let out2 = worker.sync(&p, WorkType::Sync).await?;
    let sb2 = out2.sandbox_id.ok_or("no sandbox")?;
    assert_ne!(sb1, sb2, "sandbox should have been recreated");
    let sbs = cri.list_pod_sandbox(None).await?;
    assert_eq!(sbs.len(), 1);
    assert_eq!(sbs[0].state, PodSandboxState::Ready);
    Ok(())
}

#[tokio::test]
async fn worker_state_starts_idle_then_progresses_through_syncing_to_waiting() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    assert_eq!(worker.state(), PodWorkerState::Idle);
    let p = pod("api", "u1", vec!["app"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    assert_eq!(worker.state(), PodWorkerState::Waiting);
    Ok(())
}

#[tokio::test]
async fn sync_skips_containers_that_changed_image() -> TestResult {
    // Image change is treated as no-op by Phase 1 — recorded as `[[unmapped]]`
    // (`computePodActions::ImageChanged`). We just assert a second sync with
    // a new image does not crash and does not duplicate containers.
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let mut p = pod("api", "u1", vec!["app"]);
    let _ = worker.sync(&p, WorkType::Sync).await?;
    p.spec.containers[0].image = "nginx:1.28".into();
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert_eq!(out.created_containers, [] as [std::string::String; 0]);
    let cs = cri.list_containers(None).await?;
    assert_eq!(cs.len(), 1);
    Ok(())
}

#[tokio::test]
async fn two_pods_get_independent_sandboxes() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let w1 = PodWorker::new(cri.clone());
    let w2 = PodWorker::new(cri.clone());
    let p1 = pod("a", "ua", vec!["app"]);
    let p2 = pod("b", "ub", vec!["app"]);
    let o1 = w1.sync(&p1, WorkType::Sync).await?;
    let o2 = w2.sync(&p2, WorkType::Sync).await?;
    assert_ne!(o1.sandbox_id, o2.sandbox_id);
    assert_eq!(cri.list_pod_sandbox(None).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn syncing_pod_with_no_containers_creates_only_sandbox() -> TestResult {
    let cri = Arc::new(MockCriClient::new());
    let worker = PodWorker::new(cri.clone());
    let p = pod("noctrs", "u1", vec![]);
    let out = worker.sync(&p, WorkType::Sync).await?;
    assert!(out.sandbox_id.is_some());
    assert_eq!(out.created_containers, [] as [std::string::String; 0]);
    assert_eq!(cri.list_pod_sandbox(None).await?.len(), 1);
    assert_eq!(
        cri.list_containers(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::Container; 0]
    );
    Ok(())
}
