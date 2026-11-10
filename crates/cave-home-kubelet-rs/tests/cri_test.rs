// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::cri::MockCriClient`.
//!
//! Hand-port targets:
//! - `upstream_test`: `pkg/kubelet/kuberuntime/fake_kuberuntime_manager.go::TestFake*`
//! - `upstream_test`: `pkg/kubelet/cri/remote/remote_runtime_test.go::TestVersion`
//! - `upstream_test`: `pkg/kubelet/cri/remote/remote_runtime_test.go::TestRunPodSandbox`

use cave_home_kubelet_rs::cri::types::{
    ContainerConfig, ContainerFilter, ContainerMetadata, ContainerState, ImageSpec,
    PodSandboxConfig, PodSandboxFilter, PodSandboxMetadata, PodSandboxState,
};
use cave_home_kubelet_rs::cri::{CriClient, CriError, MockCriClient};

type TestResult = Result<(), Box<dyn std::error::Error>>;

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
async fn version_returns_phase_one_string() -> TestResult {
    let cri = MockCriClient::new();
    let v = cri.version().await?;
    assert!(v.contains("cave-home"));
    Ok(())
}

#[tokio::test]
async fn run_pod_sandbox_returns_id_and_lists_it() -> TestResult {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    assert_ne!(id, "");
    let list = cri.list_pod_sandbox(None).await?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].state, PodSandboxState::Ready);
    Ok(())
}

#[tokio::test]
async fn pod_sandbox_status_returns_metadata() -> TestResult {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    let st = cri.pod_sandbox_status(&id).await?;
    assert_eq!(st.id, id);
    assert_eq!(st.metadata.uid, "u");
    assert_eq!(st.state, PodSandboxState::Ready);
    Ok(())
}

#[tokio::test]
async fn pod_sandbox_status_unknown_id_returns_not_found() -> TestResult {
    let cri = MockCriClient::new();
    let err = cri
        .pod_sandbox_status("missing")
        .await
        .err()
        .ok_or("expected an error")?;
    assert!(matches!(err, CriError::NotFound(_)));
    Ok(())
}

#[tokio::test]
async fn stop_pod_sandbox_transitions_state_to_not_ready() -> TestResult {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    cri.stop_pod_sandbox(&id).await?;
    let st = cri.pod_sandbox_status(&id).await?;
    assert_eq!(st.state, PodSandboxState::NotReady);
    Ok(())
}

#[tokio::test]
async fn remove_pod_sandbox_removes_it_from_list() -> TestResult {
    let cri = MockCriClient::new();
    let id = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    cri.stop_pod_sandbox(&id).await?;
    cri.remove_pod_sandbox(&id).await?;
    let list = cri.list_pod_sandbox(None).await?;
    assert_eq!(
        list,
        [] as [cave_home_kubelet_rs::cri::types::PodSandbox; 0]
    );
    Ok(())
}

#[tokio::test]
async fn create_container_then_start_then_list_runs() -> TestResult {
    let cri = MockCriClient::new();
    let sb = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    let cid = cri
        .create_container(&sb, container_cfg("c", "nginx"), sandbox_cfg("p", "u"))
        .await?;
    cri.start_container(&cid).await?;
    let st = cri.container_status(&cid).await?;
    assert_eq!(st.state, ContainerState::Running);
    let list = cri.list_containers(None).await?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].state, ContainerState::Running);
    Ok(())
}

#[tokio::test]
async fn stop_container_transitions_to_exited() -> TestResult {
    let cri = MockCriClient::new();
    let sb = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    let cid = cri
        .create_container(&sb, container_cfg("c", "nginx"), sandbox_cfg("p", "u"))
        .await?;
    cri.start_container(&cid).await?;
    cri.stop_container(&cid, 30).await?;
    let st = cri.container_status(&cid).await?;
    assert_eq!(st.state, ContainerState::Exited);
    Ok(())
}

#[tokio::test]
async fn remove_container_drops_it_from_list() -> TestResult {
    let cri = MockCriClient::new();
    let sb = cri.run_pod_sandbox(sandbox_cfg("p", "u")).await?;
    let cid = cri
        .create_container(&sb, container_cfg("c", "nginx"), sandbox_cfg("p", "u"))
        .await?;
    cri.stop_container(&cid, 0).await?;
    cri.remove_container(&cid).await?;
    let list = cri.list_containers(None).await?;
    assert_eq!(list, [] as [cave_home_kubelet_rs::cri::types::Container; 0]);
    Ok(())
}

#[tokio::test]
async fn list_containers_filter_by_pod_sandbox() -> TestResult {
    let cri = MockCriClient::new();
    let s1 = cri.run_pod_sandbox(sandbox_cfg("p1", "u1")).await?;
    let s2 = cri.run_pod_sandbox(sandbox_cfg("p2", "u2")).await?;
    let _ = cri
        .create_container(&s1, container_cfg("a", "i"), sandbox_cfg("p1", "u1"))
        .await?;
    let _ = cri
        .create_container(&s2, container_cfg("b", "i"), sandbox_cfg("p2", "u2"))
        .await?;
    let list = cri
        .list_containers(Some(ContainerFilter {
            pod_sandbox_id: Some(s1.clone()),
            ..Default::default()
        }))
        .await?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pod_sandbox_id, s1);
    Ok(())
}

#[tokio::test]
async fn list_pod_sandbox_filter_by_state() -> TestResult {
    let cri = MockCriClient::new();
    let a = cri.run_pod_sandbox(sandbox_cfg("a", "ua")).await?;
    let _ = cri.run_pod_sandbox(sandbox_cfg("b", "ub")).await?;
    cri.stop_pod_sandbox(&a).await?;
    let ready = cri
        .list_pod_sandbox(Some(PodSandboxFilter {
            state: Some(PodSandboxState::Ready),
            ..Default::default()
        }))
        .await?;
    assert_eq!(ready.len(), 1);
    let not_ready = cri
        .list_pod_sandbox(Some(PodSandboxFilter {
            state: Some(PodSandboxState::NotReady),
            ..Default::default()
        }))
        .await?;
    assert_eq!(not_ready.len(), 1);
    Ok(())
}

#[tokio::test]
async fn pull_image_then_image_status_returns_image() -> TestResult {
    let cri = MockCriClient::new();
    let img = ImageSpec {
        image: "nginx:1.27".into(),
    };
    let id = cri.pull_image(img.clone()).await?;
    assert_ne!(id, "");
    let got = cri.image_status(img.clone()).await?;
    assert!(got.is_some());
    let got = got.ok_or("image status missing")?;
    assert!(got.repo_tags.contains(&"nginx:1.27".to_string()));
    Ok(())
}

#[tokio::test]
async fn image_status_unknown_returns_none() -> TestResult {
    let cri = MockCriClient::new();
    let got = cri
        .image_status(ImageSpec {
            image: "missing:1".into(),
        })
        .await?;
    assert!(got.is_none());
    Ok(())
}

#[tokio::test]
async fn cannot_start_container_in_unknown_state() -> TestResult {
    let cri = MockCriClient::new();
    let err = cri
        .start_container("nope")
        .await
        .err()
        .ok_or("expected an error")?;
    assert!(matches!(err, CriError::NotFound(_)));
    Ok(())
}

#[tokio::test]
async fn list_images_returns_pulled_images() -> TestResult {
    let cri = MockCriClient::new();
    assert_eq!(
        cri.list_images(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::Image; 0]
    );
    cri.pull_image(ImageSpec {
        image: "nginx:1.27".into(),
    })
    .await?;
    cri.pull_image(ImageSpec {
        image: "redis:7".into(),
    })
    .await?;
    let all = cri.list_images(None).await?;
    assert_eq!(all.len(), 2);

    // Filtering by an image reference narrows the result.
    let only = cri
        .list_images(Some(ImageSpec {
            image: "redis:7".into(),
        }))
        .await?;
    assert_eq!(only.len(), 1);
    assert!(only[0].repo_tags.contains(&"redis:7".to_string()));
    Ok(())
}

#[tokio::test]
async fn remove_image_drops_it_from_status_and_list() -> TestResult {
    let cri = MockCriClient::new();
    let spec = ImageSpec {
        image: "nginx:1.27".into(),
    };
    cri.pull_image(spec.clone()).await?;
    assert!(cri.image_status(spec.clone()).await?.is_some());

    cri.remove_image(spec.clone()).await?;
    assert!(cri.image_status(spec.clone()).await?.is_none());
    assert_eq!(
        cri.list_images(None).await?,
        [] as [cave_home_kubelet_rs::cri::types::Image; 0]
    );

    // Removing an absent image is idempotent (matches CRI semantics).
    cri.remove_image(spec).await?;
    Ok(())
}

#[tokio::test]
async fn image_fs_info_reports_usage() -> TestResult {
    let cri = MockCriClient::new();
    cri.pull_image(ImageSpec {
        image: "nginx:1.27".into(),
    })
    .await?;
    let fs = cri.image_fs_info().await?;
    assert_ne!(
        fs,
        [] as [cave_home_kubelet_rs::cri::types::FilesystemUsage; 0]
    );
    assert_ne!(fs[0].mountpoint, "");
    Ok(())
}
