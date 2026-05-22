// SPDX-License-Identifier: Apache-2.0
//! CRI Container tests.
//!
//! Mirrors `internal/cri/store/container/container_test.go` (store
//! CRUD) and `internal/cri/server/container_*_test.go` (handler-level
//! state-machine assertions).

use std::time::SystemTime;

use cave_home_containerd_rs::cri::types::{
    ContainerMetadata, ContainerState, ContainerStatus, SandboxMetadata, SandboxState,
    SandboxStatus,
};
use cave_home_containerd_rs::cri::{
    Container, ContainerStore, CriError, RuntimeServer, Sandbox, SandboxStore,
};
use cave_home_containerd_rs::runtime_v1 as pb;
use cave_home_containerd_rs::runtime_v1::runtime_service_server::RuntimeService;
use tonic::Request;

fn fixture_c(id: &str, sandbox_id: &str) -> Container {
    Container {
        metadata: ContainerMetadata {
            id: id.to_owned(),
            sandbox_id: sandbox_id.to_owned(),
            name: format!("c-{id}"),
            image: "alpine:3".to_owned(),
            runtime_handler: String::new(),
            created_at: SystemTime::now(),
        },
        status: ContainerStatus::default(),
    }
}

#[test]
fn test_container_store_add_then_get() {
    let s = ContainerStore::new();
    s.add(fixture_c("c1", "sb1")).unwrap();
    assert_eq!(s.get("c1").unwrap().metadata.sandbox_id, "sb1");
}

#[test]
fn test_container_store_add_duplicate_errors() {
    let s = ContainerStore::new();
    s.add(fixture_c("c1", "sb1")).unwrap();
    let err = s.add(fixture_c("c1", "sb1")).unwrap_err();
    assert!(matches!(err, CriError::AlreadyExists(_)));
}

#[test]
fn test_container_store_list_for_sandbox_filters() {
    let s = ContainerStore::new();
    s.add(fixture_c("c1", "sbA")).unwrap();
    s.add(fixture_c("c2", "sbA")).unwrap();
    s.add(fixture_c("c3", "sbB")).unwrap();
    assert_eq!(s.list_for_sandbox("sbA").len(), 2);
    assert_eq!(s.list_for_sandbox("sbB").len(), 1);
    assert_eq!(s.list_for_sandbox("sbZ").len(), 0);
}

#[test]
fn test_container_store_delete_refuses_running_container() {
    let s = ContainerStore::new();
    s.add(fixture_c("c1", "sb1")).unwrap();
    s.update_status("c1", |st| st.state = ContainerState::Running).unwrap();
    let err = s.delete("c1").unwrap_err();
    assert!(matches!(err, CriError::FailedPrecondition(_)));
}

// ----- gRPC handler tests -------------------------------------------

async fn server_with_sandbox() -> (RuntimeServer, String) {
    let server = RuntimeServer::new(SandboxStore::new(), ContainerStore::new());
    let sb = Sandbox {
        metadata: SandboxMetadata {
            id: "sb-fixture".to_owned(),
            name: "p1".to_owned(),
            uid: "u1".to_owned(),
            namespace: "default".to_owned(),
            runtime_handler: "runc".to_owned(),
            net_ns_path: String::new(),
            process_label: String::new(),
            created_at: SystemTime::now(),
        },
        status: SandboxStatus { state: SandboxState::Ready, state_changed_at: SystemTime::now() },
    };
    server.sandboxes().add(sb).unwrap();
    (server, "sb-fixture".to_owned())
}

fn make_create_req(sandbox_id: &str, name: &str) -> pb::CreateContainerRequest {
    pb::CreateContainerRequest {
        pod_sandbox_id: sandbox_id.to_owned(),
        config: Some(pb::ContainerConfig {
            metadata: Some(pb::ContainerMetadata { name: name.to_owned(), attempt: 0 }),
            image: Some(pb::ImageSpec {
                image: "alpine:3".to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        sandbox_config: None,
    }
}

#[tokio::test]
async fn test_create_container_returns_id_and_records_state_created() {
    let (s, sb_id) = server_with_sandbox().await;
    let resp = s
        .create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap()
        .into_inner();
    let id = resp.container_id;
    assert!(!id.is_empty());
    let c = s.containers().get(&id).unwrap();
    assert_eq!(c.status.state, ContainerState::Created);
    assert_eq!(c.metadata.image, "alpine:3");
}

#[tokio::test]
async fn test_create_container_rejects_unknown_sandbox() {
    let s = RuntimeServer::new(SandboxStore::new(), ContainerStore::new());
    let err = s
        .create_container(Request::new(make_create_req("ghost", "c1")))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn test_start_container_transitions_created_to_running() {
    let (s, sb_id) = server_with_sandbox().await;
    let id = s
        .create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap()
        .into_inner()
        .container_id;
    s.start_container(Request::new(pb::StartContainerRequest { container_id: id.clone() }))
        .await
        .unwrap();
    assert_eq!(s.containers().get(&id).unwrap().status.state, ContainerState::Running);
}

#[tokio::test]
async fn test_stop_container_transitions_running_to_exited() {
    let (s, sb_id) = server_with_sandbox().await;
    let id = s
        .create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap()
        .into_inner()
        .container_id;
    s.start_container(Request::new(pb::StartContainerRequest { container_id: id.clone() }))
        .await
        .unwrap();
    s.stop_container(Request::new(pb::StopContainerRequest {
        container_id: id.clone(),
        timeout: 0,
    }))
    .await
    .unwrap();
    assert_eq!(s.containers().get(&id).unwrap().status.state, ContainerState::Exited);
}

#[tokio::test]
async fn test_remove_container_after_stop_clears_record() {
    let (s, sb_id) = server_with_sandbox().await;
    let id = s
        .create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap()
        .into_inner()
        .container_id;
    s.start_container(Request::new(pb::StartContainerRequest { container_id: id.clone() }))
        .await
        .unwrap();
    s.stop_container(Request::new(pb::StopContainerRequest {
        container_id: id.clone(),
        timeout: 0,
    }))
    .await
    .unwrap();
    s.remove_container(Request::new(pb::RemoveContainerRequest {
        container_id: id.clone(),
    }))
    .await
    .unwrap();
    assert!(s.containers().get(&id).is_err());
}

#[tokio::test]
async fn test_remove_container_refuses_running() {
    let (s, sb_id) = server_with_sandbox().await;
    let id = s
        .create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap()
        .into_inner()
        .container_id;
    s.start_container(Request::new(pb::StartContainerRequest { container_id: id.clone() }))
        .await
        .unwrap();
    let err = s
        .remove_container(Request::new(pb::RemoveContainerRequest { container_id: id }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn test_container_status_returns_full_record() {
    let (s, sb_id) = server_with_sandbox().await;
    let id = s
        .create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap()
        .into_inner()
        .container_id;
    let r = s
        .container_status(Request::new(pb::ContainerStatusRequest {
            container_id: id.clone(),
            verbose: false,
        }))
        .await
        .unwrap()
        .into_inner();
    let st = r.status.unwrap();
    assert_eq!(st.id, id);
    assert_eq!(st.state, pb::ContainerState::ContainerCreated as i32);
}

#[tokio::test]
async fn test_list_containers_returns_all() {
    let (s, sb_id) = server_with_sandbox().await;
    for n in &["c1", "c2"] {
        s.create_container(Request::new(make_create_req(&sb_id, n))).await.unwrap();
    }
    let resp = s
        .list_containers(Request::new(pb::ListContainersRequest { filter: None }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.containers.len(), 2);
}

#[tokio::test]
async fn test_remove_pod_sandbox_refuses_when_containers_present() {
    let (s, sb_id) = server_with_sandbox().await;
    s.create_container(Request::new(make_create_req(&sb_id, "c1")))
        .await
        .unwrap();
    let err = s
        .remove_pod_sandbox(Request::new(pb::RemovePodSandboxRequest {
            pod_sandbox_id: sb_id,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}
