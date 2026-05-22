// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::cri::MockCriClient`.
//!
//! Hand-port targets:
//! - upstream_test: `pkg/kubelet/kuberuntime/fake_kuberuntime_manager.go::TestFake*`
//! - upstream_test: `pkg/kubelet/cri/remote/remote_runtime_test.go::TestVersion`
//! - upstream_test: `pkg/kubelet/cri/remote/remote_runtime_test.go::TestRunPodSandbox`

use cave_home_kubelet_rs::cri::types::{
    ContainerConfig, ContainerFilter, ContainerMetadata, ContainerState, ImageSpec,
    PodSandboxConfig, PodSandboxFilter, PodSandboxMetadata, PodSandboxState,
};
use cave_home_kubelet_rs::cri::{CriClient, CriError, MockCriClient};

fn sandbox_cfg(name: &str, uid: &str) -> PodSandboxConfig {
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

fn container_cfg(name: &str, image: &str) -> ContainerConfig {
    ContainerConfig {
        metadata: ContainerMetadata {
            name: name.into(),
            attempt: 0,
        },
        image: ImageSpec {
            image: image.into(),
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn version_returns_phase_one_string() {
    let cri = MockCriClient::new();
    let v = cri.version().await.unwrap();
    assert!(v.contains("cave-home"));
}

#[tokio::test]
async fn run_pod_sandbox_returns_id_and_lists_it() {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    assert!(!id.is_empty());
    let list = cri.list_pod_sandbox(None).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].state, PodSandboxState::Ready);
}

#[tokio::test]
async fn pod_sandbox_status_returns_metadata() {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    let st = cri.pod_sandbox_status(&id).await.unwrap();
    assert_eq!(st.id, id);
    assert_eq!(st.metadata.uid, "u");
    assert_eq!(st.state, PodSandboxState::Ready);
}

#[tokio::test]
async fn pod_sandbox_status_unknown_id_returns_not_found() {
    let cri = MockCriClient::new();
    let err = cri.pod_sandbox_status("missing").await.unwrap_err();
    assert!(matches!(err, CriError::NotFound(_)));
}

#[tokio::test]
async fn stop_pod_sandbox_transitions_state_to_not_ready() {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    cri.stop_pod_sandbox(&id).await.unwrap();
    let st = cri.pod_sandbox_status(&id).await.unwrap();
    assert_eq!(st.state, PodSandboxState::NotReady);
}

#[tokio::test]
async fn remove_pod_sandbox_removes_it_from_list() {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    cri.stop_pod_sandbox(&id).await.unwrap();
    cri.remove_pod_sandbox(&id).await.unwrap();
    let list = cri.list_pod_sandbox(None).await.unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn create_container_then_start_then_list_runs() {
    let cri = MockCriClient::new();
    let sb = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, container_cfg("c", "nginx"), sandbox_cfg("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();
    let st = cri.container_status(&cid).await.unwrap();
    assert_eq!(st.state, ContainerState::Running);
    let list = cri.list_containers(None).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].state, ContainerState::Running);
}

#[tokio::test]
async fn stop_container_transitions_to_exited() {
    let cri = MockCriClient::new();
    let sb = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, container_cfg("c", "nginx"), sandbox_cfg("p", "u"))
        .await
        .unwrap();
    cri.start_container(&cid).await.unwrap();
    cri.stop_container(&cid, 30).await.unwrap();
    let st = cri.container_status(&cid).await.unwrap();
    assert_eq!(st.state, ContainerState::Exited);
}

#[tokio::test]
async fn remove_container_drops_it_from_list() {
    let cri = MockCriClient::new();
    let sb = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await.unwrap();
    let cid = cri
        .create_container(&sb, container_cfg("c", "nginx"), sandbox_cfg("p", "u"))
        .await
        .unwrap();
    cri.stop_container(&cid, 0).await.unwrap();
    cri.remove_container(&cid).await.unwrap();
    let list = cri.list_containers(None).await.unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn list_containers_filter_by_pod_sandbox() {
    let cri = MockCriClient::new();
    let s1 = cri.run_pod_sandbox(sandbox_cfg("p1", "u1")).await.unwrap();
    let s2 = cri.run_pod_sandbox(sandbox_cfg("p2", "u2")).await.unwrap();
    let _ = cri
        .create_container(&s1, container_cfg("a", "i"), sandbox_cfg("p1", "u1"))
        .await
        .unwrap();
    let _ = cri
        .create_container(&s2, container_cfg("b", "i"), sandbox_cfg("p2", "u2"))
        .await
        .unwrap();
    let list = cri
        .list_containers(Some(ContainerFilter {
            pod_sandbox_id: Some(s1.clone()),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pod_sandbox_id, s1);
}

#[tokio::test]
async fn list_pod_sandbox_filter_by_state() {
    let cri = MockCriClient::new();
    let a = cri.run_pod_sandbox(sandbox_cfg("a", "ua")).await.unwrap();
    let _ = cri.run_pod_sandbox(sandbox_cfg("b", "ub")).await.unwrap();
    cri.stop_pod_sandbox(&a).await.unwrap();
    let ready = cri
        .list_pod_sandbox(Some(PodSandboxFilter {
            state: Some(PodSandboxState::Ready),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(ready.len(), 1);
    let not_ready = cri
        .list_pod_sandbox(Some(PodSandboxFilter {
            state: Some(PodSandboxState::NotReady),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(not_ready.len(), 1);
}

#[tokio::test]
async fn pull_image_then_image_status_returns_image() {
    let cri = MockCriClient::new();
    let img = ImageSpec {
        image: "nginx:1.27".into(),
    };
    let id = cri.pull_image(img.clone()).await.unwrap();
    assert!(!id.is_empty());
    let got = cri.image_status(img.clone()).await.unwrap();
    assert!(got.is_some());
    let got = got.unwrap();
    assert!(got.repo_tags.contains(&"nginx:1.27".to_string()));
}

#[tokio::test]
async fn image_status_unknown_returns_none() {
    let cri = MockCriClient::new();
    let got = cri
        .image_status(ImageSpec {
            image: "missing:1".into(),
        })
        .await
        .unwrap();
    assert!(got.is_none());
}

#[tokio::test]
async fn cannot_start_container_in_unknown_state() {
    let cri = MockCriClient::new();
    let err = cri.start_container("nope").await.unwrap_err();
    assert!(matches!(err, CriError::NotFound(_)));
}
