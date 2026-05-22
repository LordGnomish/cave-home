// SPDX-License-Identifier: Apache-2.0
//! CRI PodSandbox tests.
//!
//! Direct ports of upstream cases from
//! `internal/cri/store/sandbox/sandbox_test.go` (store CRUD) and
//! `internal/cri/server/sandbox_*_test.go` (RPC handlers exercised
//! against the in-memory backend).

use std::time::SystemTime;

use cave_home_containerd_rs::cri::types::{SandboxMetadata, SandboxState, SandboxStatus};
use cave_home_containerd_rs::cri::{
    ContainerStore, CriError, RuntimeServer, Sandbox, SandboxStore,
};
use cave_home_containerd_rs::runtime_v1 as pb;
use cave_home_containerd_rs::runtime_v1::runtime_service_server::RuntimeService;
use tonic::Request;

fn fixture_sb(id: &str, name: &str) -> Sandbox {
    Sandbox {
        metadata: SandboxMetadata {
            id: id.to_owned(),
            name: name.to_owned(),
            uid: "uid".to_owned(),
            namespace: "default".to_owned(),
            runtime_handler: "runc".to_owned(),
            net_ns_path: String::new(),
            process_label: String::new(),
            created_at: SystemTime::now(),
        },
        status: SandboxStatus { state: SandboxState::Ready, state_changed_at: SystemTime::now() },
    }
}

#[test]
fn test_sandbox_store_add_then_get() {
    let s = SandboxStore::new();
    s.add(fixture_sb("sb1", "n1")).unwrap();
    let got = s.get("sb1").unwrap();
    assert_eq!(got.metadata.name, "n1");
}

#[test]
fn test_sandbox_store_add_duplicate_errors() {
    let s = SandboxStore::new();
    s.add(fixture_sb("sb1", "n1")).unwrap();
    let err = s.add(fixture_sb("sb1", "n2")).unwrap_err();
    assert!(matches!(err, CriError::AlreadyExists(_)));
}

#[test]
fn test_sandbox_store_get_missing_is_not_found() {
    let s = SandboxStore::new();
    let err = s.get("nope").unwrap_err();
    assert!(matches!(err, CriError::NotFound(_)));
}

#[test]
fn test_sandbox_store_list_returns_all() {
    let s = SandboxStore::new();
    s.add(fixture_sb("a", "a")).unwrap();
    s.add(fixture_sb("b", "b")).unwrap();
    s.add(fixture_sb("c", "c")).unwrap();
    assert_eq!(s.list().len(), 3);
}

#[test]
fn test_sandbox_store_update_status() {
    let s = SandboxStore::new();
    s.add(fixture_sb("sb1", "n1")).unwrap();
    s.update_status("sb1", |st| st.state = SandboxState::NotReady).unwrap();
    assert_eq!(s.get("sb1").unwrap().status.state, SandboxState::NotReady);
}

#[test]
fn test_sandbox_store_delete_is_silent_for_missing() {
    let s = SandboxStore::new();
    s.delete("nope"); // upstream silently returns; no panic, no err
    s.add(fixture_sb("sb1", "n1")).unwrap();
    s.delete("sb1");
    assert!(matches!(s.get("sb1").unwrap_err(), CriError::NotFound(_)));
}

// ----- gRPC handler tests (RuntimeServer) ---------------------------

fn server() -> RuntimeServer {
    RuntimeServer::new(SandboxStore::new(), ContainerStore::new())
}

fn make_run_req(name: &str) -> pb::RunPodSandboxRequest {
    pb::RunPodSandboxRequest {
        config: Some(pb::PodSandboxConfig {
            metadata: Some(pb::PodSandboxMetadata {
                name: name.to_owned(),
                uid: "uid".to_owned(),
                namespace: "default".to_owned(),
                attempt: 0,
            }),
            ..Default::default()
        }),
        runtime_handler: "runc".to_owned(),
    }
}

#[tokio::test]
async fn test_run_pod_sandbox_creates_record_and_returns_id() {
    let s = server();
    let resp = s.run_pod_sandbox(Request::new(make_run_req("p1"))).await.unwrap();
    let id = resp.into_inner().pod_sandbox_id;
    assert!(!id.is_empty());
    assert!(s.sandboxes().get(&id).is_ok());
}

#[tokio::test]
async fn test_run_pod_sandbox_rejects_missing_config() {
    let s = server();
    let req = pb::RunPodSandboxRequest { config: None, runtime_handler: String::new() };
    let err = s.run_pod_sandbox(Request::new(req)).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn test_pod_sandbox_status_round_trip() {
    let s = server();
    let id = s
        .run_pod_sandbox(Request::new(make_run_req("p1")))
        .await
        .unwrap()
        .into_inner()
        .pod_sandbox_id;
    let status = s
        .pod_sandbox_status(Request::new(pb::PodSandboxStatusRequest {
            pod_sandbox_id: id.clone(),
            verbose: false,
        }))
        .await
        .unwrap()
        .into_inner();
    let st = status.status.unwrap();
    assert_eq!(st.id, id);
    assert_eq!(st.state, pb::PodSandboxState::SandboxReady as i32);
}

#[tokio::test]
async fn test_stop_pod_sandbox_marks_notready_and_is_idempotent() {
    let s = server();
    let id = s
        .run_pod_sandbox(Request::new(make_run_req("p1")))
        .await
        .unwrap()
        .into_inner()
        .pod_sandbox_id;
    s.stop_pod_sandbox(Request::new(pb::StopPodSandboxRequest {
        pod_sandbox_id: id.clone(),
    }))
    .await
    .unwrap();
    assert_eq!(s.sandboxes().get(&id).unwrap().status.state, SandboxState::NotReady);
    // Second stop is a no-op (idempotent).
    s.stop_pod_sandbox(Request::new(pb::StopPodSandboxRequest {
        pod_sandbox_id: id,
    }))
    .await
    .unwrap();
}

#[tokio::test]
async fn test_remove_pod_sandbox_clears_record() {
    let s = server();
    let id = s
        .run_pod_sandbox(Request::new(make_run_req("p1")))
        .await
        .unwrap()
        .into_inner()
        .pod_sandbox_id;
    s.remove_pod_sandbox(Request::new(pb::RemovePodSandboxRequest {
        pod_sandbox_id: id.clone(),
    }))
    .await
    .unwrap();
    assert!(s.sandboxes().get(&id).is_err());
}

#[tokio::test]
async fn test_list_pod_sandbox_returns_all_records() {
    let s = server();
    for n in &["p1", "p2", "p3"] {
        s.run_pod_sandbox(Request::new(make_run_req(n))).await.unwrap();
    }
    let resp = s
        .list_pod_sandbox(Request::new(pb::ListPodSandboxRequest { filter: None }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.items.len(), 3);
}

#[tokio::test]
async fn test_version_returns_runtime_name() {
    let s = server();
    let v = s.version(Request::new(pb::VersionRequest { version: String::new() }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(v.runtime_name, "cave-home-containerd-rs");
    assert_eq!(v.runtime_api_version, "v1");
}

#[tokio::test]
async fn test_status_reports_runtime_ready_and_network_phase1b() {
    let s = server();
    let r = s
        .status(Request::new(pb::StatusRequest { verbose: false }))
        .await
        .unwrap()
        .into_inner();
    let conds = r.status.unwrap().conditions;
    assert!(conds.iter().any(|c| c.r#type == "RuntimeReady" && c.status));
    assert!(conds.iter().any(|c| c.r#type == "NetworkReady" && !c.status));
}

#[tokio::test]
async fn test_streaming_rpc_returns_unimplemented_grpc_status() {
    // Phase 1 honestly returns gRPC-protocol-level Unimplemented for
    // streaming RPCs — that is NOT a Rust panic.
    let s = server();
    let err = s
        .exec(Request::new(pb::ExecRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unimplemented);
}
