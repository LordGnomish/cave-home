// SPDX-License-Identifier: Apache-2.0
//! End-to-end test against a **real** containerd CRI socket.
//!
//! `#[ignore]` by default — it requires a running containerd on the host (with
//! the CRI plugin, a pause image, and CNI configured). Run it explicitly:
//!
//! ```text
//! # default socket /run/containerd/containerd.sock
//! cargo test -p cave-home-kubelet-rs --features remote-cri \
//!     --test cri_remote_containerd_test -- --ignored --nocapture
//!
//! # or point at a custom socket
//! CAVE_CRI_CONTAINERD_SOCK=/var/run/containerd/containerd.sock \
//!     cargo test ... -- --ignored --nocapture
//! ```
//!
//! NOTE: this was authored on a macOS host with no containerd, so it has not
//! been executed here — it is wired to run unchanged on a Linux node that has
//! containerd. If the socket is absent the test logs and returns (a no-op
//! pass) rather than failing, so a stray `--ignored` run elsewhere is benign.
#![cfg(feature = "remote-cri")]

use std::path::Path;

use cave_home_kubelet_rs::cri::types as t;
use cave_home_kubelet_rs::cri::CriClient;
use cave_home_kubelet_rs::cri::remote::RemoteCriClient;

fn socket_path() -> String {
    std::env::var("CAVE_CRI_CONTAINERD_SOCK")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_owned())
}

#[tokio::test]
#[ignore = "requires a running containerd; see module docs"]
async fn real_containerd_pod_bringup() {
    let sock = socket_path();
    if !Path::new(&sock).exists() {
        eprintln!("SKIP: no containerd socket at {sock} (set CAVE_CRI_CONTAINERD_SOCK)");
        return;
    }

    let client = RemoteCriClient::connect_uds(&sock)
        .await
        .expect("connect to containerd CRI socket");

    // 1. Version handshake — proves the gRPC channel + CRI plugin are live.
    let version = client.version().await.expect("Version RPC");
    eprintln!("containerd runtime version: {version}");
    assert!(!version.is_empty());

    // 2. RunPodSandbox.
    let sandbox_cfg = t::PodSandboxConfig {
        metadata: t::PodSandboxMetadata {
            name: "cave-home-e2e".into(),
            uid: "cave-home-e2e-uid".into(),
            namespace: "default".into(),
            attempt: 0,
        },
        log_directory: "/tmp/cave-home-e2e".into(),
        linux: t::LinuxPodSandboxConfig {
            cgroup_parent: String::new(),
            namespace_options: t::NamespaceOption::default(),
        },
        ..Default::default()
    };
    let sandbox_id = client
        .run_pod_sandbox(sandbox_cfg.clone())
        .await
        .expect("RunPodSandbox");
    eprintln!("sandbox: {sandbox_id}");

    // 3. Pull the workload image, then CreateContainer + StartContainer.
    let image = t::ImageSpec {
        image: "docker.io/library/busybox:latest".into(),
    };
    client.pull_image(image.clone()).await.expect("PullImage");

    let container_cfg = t::ContainerConfig {
        metadata: t::ContainerMetadata {
            name: "busybox".into(),
            attempt: 0,
        },
        image: image.clone(),
        command: vec!["sleep".into()],
        args: vec!["3600".into()],
        log_path: "busybox/0.log".into(),
        ..Default::default()
    };
    let container_id = client
        .create_container(&sandbox_id, container_cfg, sandbox_cfg)
        .await
        .expect("CreateContainer");
    eprintln!("container: {container_id}");

    client
        .start_container(&container_id)
        .await
        .expect("StartContainer");

    let status = client
        .container_status(&container_id)
        .await
        .expect("ContainerStatus");
    assert_eq!(status.state, t::ContainerState::Running);

    // 4. Teardown — best effort.
    let _ = client.stop_container(&container_id, 0).await;
    let _ = client.remove_container(&container_id).await;
    let _ = client.stop_pod_sandbox(&sandbox_id).await;
    let _ = client.remove_pod_sandbox(&sandbox_id).await;
    eprintln!("real containerd bring-up OK");
}
