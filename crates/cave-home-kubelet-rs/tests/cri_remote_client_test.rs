// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `RemoteCriClient` against a real in-process CRI gRPC
//! server over a Unix socket. These exercise the full client stack: native ->
//! proto marshalling, HTTP/2 transport, and proto -> native demarshalling.
#![cfg(feature = "remote-cri")]

mod common;

use cave_home_kubelet_rs::cri::remote::RemoteCriClient;
use cave_home_kubelet_rs::cri::types as t;
use cave_home_kubelet_rs::cri::{CriClient, CriError};

use common::start_mock_cri_server;

async fn connect(server: &common::MockServerHandle) -> RemoteCriClient {
    RemoteCriClient::connect_uds(&server.socket_path)
        .await
        .expect("connect to mock CRI socket")
}

fn sandbox_config(name: &str) -> t::PodSandboxConfig {
    t::PodSandboxConfig {
        metadata: t::PodSandboxMetadata {
            name: name.into(),
            uid: format!("uid-{name}"),
            namespace: "default".into(),
            attempt: 0,
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn version_round_trips_over_grpc() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;
    assert_eq!(client.version().await.unwrap(), "1.7.0-mock");
}

#[tokio::test]
async fn run_pod_sandbox_returns_id_and_persists_on_server() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;

    let id = client.run_pod_sandbox(sandbox_config("web")).await.unwrap();
    assert!(id.starts_with("sb-"));
    assert_eq!(server.runtime.sandbox_count(), 1);

    let status = client.pod_sandbox_status(&id).await.unwrap();
    assert_eq!(status.id, id);
    assert_eq!(status.metadata.name, "web");
    assert_eq!(status.state, t::PodSandboxState::Ready);
}

#[tokio::test]
async fn list_pod_sandbox_honours_state_filter() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;
    let id = client.run_pod_sandbox(sandbox_config("web")).await.unwrap();

    let all = client.list_pod_sandbox(None).await.unwrap();
    assert_eq!(all.len(), 1);

    let ready = client
        .list_pod_sandbox(Some(t::PodSandboxFilter {
            id: None,
            state: Some(t::PodSandboxState::Ready),
        }))
        .await
        .unwrap();
    assert_eq!(ready.len(), 1);

    let not_ready = client
        .list_pod_sandbox(Some(t::PodSandboxFilter {
            id: None,
            state: Some(t::PodSandboxState::NotReady),
        }))
        .await
        .unwrap();
    assert!(not_ready.is_empty());

    // Stopping flips the state; the NotReady filter now matches.
    client.stop_pod_sandbox(&id).await.unwrap();
    let not_ready = client
        .list_pod_sandbox(Some(t::PodSandboxFilter {
            id: None,
            state: Some(t::PodSandboxState::NotReady),
        }))
        .await
        .unwrap();
    assert_eq!(not_ready.len(), 1);
}

#[tokio::test]
async fn missing_sandbox_status_maps_to_not_found() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;
    let err = client.pod_sandbox_status("sb-nope").await.unwrap_err();
    assert!(matches!(err, CriError::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn pull_image_then_image_status() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;

    let spec = t::ImageSpec {
        image: "nginx:1.27".into(),
    };
    let image_ref = client.pull_image(spec.clone()).await.unwrap();
    assert!(image_ref.contains("nginx:1.27"));

    let status = client.image_status(spec).await.unwrap().expect("image present");
    assert_eq!(status.repo_tags, vec!["nginx:1.27".to_owned()]);

    let absent = client
        .image_status(t::ImageSpec {
            image: "redis:7".into(),
        })
        .await
        .unwrap();
    assert!(absent.is_none());
}

#[tokio::test]
async fn list_remove_image_round_trips_over_grpc() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;

    client
        .pull_image(t::ImageSpec {
            image: "nginx:1.27".into(),
        })
        .await
        .unwrap();
    client
        .pull_image(t::ImageSpec {
            image: "redis:7".into(),
        })
        .await
        .unwrap();

    let all = client.list_images(None).await.unwrap();
    assert_eq!(all.len(), 2);

    let only = client
        .list_images(Some(t::ImageSpec {
            image: "redis:7".into(),
        }))
        .await
        .unwrap();
    assert_eq!(only.len(), 1);
    assert!(only[0].repo_tags.contains(&"redis:7".to_string()));

    client
        .remove_image(t::ImageSpec {
            image: "redis:7".into(),
        })
        .await
        .unwrap();
    assert_eq!(client.list_images(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn image_fs_info_round_trips_over_grpc() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;
    let fs = client.image_fs_info().await.unwrap();
    assert!(!fs.is_empty());
    assert_eq!(fs[0].mountpoint, "/var/lib/containerd");
    assert!(fs[0].used_bytes > 0);
}

/// The headline acceptance test: a full pod-bringup sequence driven entirely
/// over gRPC — RunPodSandbox -> CreateContainer -> StartContainer ->
/// ContainerStatus -> ListContainers.
#[tokio::test]
async fn end_to_end_pod_bringup_over_grpc() {
    let server = start_mock_cri_server().await;
    let client = connect(&server).await;

    let sandbox_cfg = sandbox_config("web");
    let sandbox_id = client.run_pod_sandbox(sandbox_cfg.clone()).await.unwrap();

    let container_cfg = t::ContainerConfig {
        metadata: t::ContainerMetadata {
            name: "app".into(),
            attempt: 0,
        },
        image: t::ImageSpec {
            image: "nginx:1.27".into(),
        },
        command: vec!["/docker-entrypoint.sh".into()],
        args: vec!["nginx".into(), "-g".into(), "daemon off;".into()],
        ..Default::default()
    };
    let container_id = client
        .create_container(&sandbox_id, container_cfg, sandbox_cfg)
        .await
        .unwrap();
    assert!(container_id.starts_with("c-"));

    // Created but not yet running.
    let st = client.container_status(&container_id).await.unwrap();
    assert_eq!(st.state, t::ContainerState::Created);
    assert_eq!(st.metadata.name, "app");
    assert_eq!(st.image.image, "nginx:1.27");

    client.start_container(&container_id).await.unwrap();
    let st = client.container_status(&container_id).await.unwrap();
    assert_eq!(st.state, t::ContainerState::Running);
    assert!(st.started_at > 0);

    // Listed under its sandbox and as Running.
    let listed = client
        .list_containers(Some(t::ContainerFilter {
            id: None,
            pod_sandbox_id: Some(sandbox_id.clone()),
            state: Some(t::ContainerState::Running),
        }))
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, container_id);
    assert_eq!(listed[0].pod_sandbox_id, sandbox_id);

    // Stop + remove tears it down.
    client.stop_container(&container_id, 0).await.unwrap();
    let st = client.container_status(&container_id).await.unwrap();
    assert_eq!(st.state, t::ContainerState::Exited);

    client.remove_container(&container_id).await.unwrap();
    assert_eq!(server.runtime.container_count(), 0);

    client.remove_pod_sandbox(&sandbox_id).await.unwrap();
    assert_eq!(server.runtime.sandbox_count(), 0);
}
