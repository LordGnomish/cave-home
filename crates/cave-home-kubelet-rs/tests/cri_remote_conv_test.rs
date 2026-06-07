// SPDX-License-Identifier: Apache-2.0
//! Conversions between the hand-ported CRI type subset (`cri::types`) and the
//! generated wire types (`cri::remote::proto`).
//!
//! Line-by-line analogue of the `to*`/`from*` helpers in
//! `k8s.io/cri-api` and `pkg/kubelet/kuberuntime` that marshal kubelet structs
//! onto the CRI proto. Runs only under the `remote-cri` feature.
#![cfg(feature = "remote-cri")]

use std::collections::BTreeMap;

use cave_home_kubelet_rs::cri::remote::proto;
use cave_home_kubelet_rs::cri::types as t;

fn btree(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn pod_sandbox_config_to_proto_maps_every_phase1_field() {
    let cfg = t::PodSandboxConfig {
        metadata: t::PodSandboxMetadata {
            name: "web".into(),
            uid: "uid-1".into(),
            namespace: "default".into(),
            attempt: 2,
        },
        hostname: "web-host".into(),
        log_directory: "/var/log/pods/default_web_uid-1".into(),
        labels: btree(&[("app", "web")]),
        annotations: btree(&[("a", "b")]),
        linux: t::LinuxPodSandboxConfig {
            cgroup_parent: "/kubepods/poduid-1".into(),
            namespace_options: t::NamespaceOption {
                network: t::NamespaceMode::Node,
                pid: t::NamespaceMode::Container,
                ipc: t::NamespaceMode::Pod,
            },
        },
    };

    let p = proto::PodSandboxConfig::from(cfg);

    let md = p.metadata.expect("metadata");
    assert_eq!(md.name, "web");
    assert_eq!(md.uid, "uid-1");
    assert_eq!(md.namespace, "default");
    assert_eq!(md.attempt, 2);
    assert_eq!(p.hostname, "web-host");
    assert_eq!(p.log_directory, "/var/log/pods/default_web_uid-1");
    assert_eq!(p.labels.get("app").map(String::as_str), Some("web"));
    assert_eq!(p.annotations.get("a").map(String::as_str), Some("b"));

    let linux = p.linux.expect("linux");
    assert_eq!(linux.cgroup_parent, "/kubepods/poduid-1");
    let ns = linux
        .security_context
        .expect("security_context")
        .namespace_options
        .expect("namespace_options");
    assert_eq!(ns.network, proto::NamespaceMode::Node as i32);
    assert_eq!(ns.pid, proto::NamespaceMode::Container as i32);
    assert_eq!(ns.ipc, proto::NamespaceMode::Pod as i32);
}

#[test]
fn sandbox_state_enum_round_trips_with_proto_numbering() {
    // proto: SANDBOX_READY = 0, SANDBOX_NOTREADY = 1 (note the inversion vs the
    // native enum's declaration order — the mapping must be by meaning, not by
    // ordinal).
    assert_eq!(
        proto::PodSandboxState::from(t::PodSandboxState::Ready) as i32,
        proto::PodSandboxState::SandboxReady as i32
    );
    assert_eq!(
        proto::PodSandboxState::from(t::PodSandboxState::NotReady) as i32,
        proto::PodSandboxState::SandboxNotready as i32
    );
    assert_eq!(
        t::PodSandboxState::from(proto::PodSandboxState::SandboxReady),
        t::PodSandboxState::Ready
    );
    assert_eq!(
        t::PodSandboxState::from(proto::PodSandboxState::SandboxNotready),
        t::PodSandboxState::NotReady
    );
}

#[test]
fn proto_pod_sandbox_to_native() {
    let p = proto::PodSandbox {
        id: "sb-1".into(),
        metadata: Some(proto::PodSandboxMetadata {
            name: "web".into(),
            uid: "uid-1".into(),
            namespace: "default".into(),
            attempt: 0,
        }),
        state: proto::PodSandboxState::SandboxReady as i32,
        created_at: 1_700_000_000,
        labels: [("app".to_owned(), "web".to_owned())].into_iter().collect(),
        ..Default::default()
    };

    let n = t::PodSandbox::from(p);
    assert_eq!(n.id, "sb-1");
    assert_eq!(n.metadata.name, "web");
    assert_eq!(n.state, t::PodSandboxState::Ready);
    assert_eq!(n.created_at, 1_700_000_000);
    assert_eq!(n.labels.get("app").map(String::as_str), Some("web"));
}

#[test]
fn proto_pod_sandbox_status_to_native() {
    let p = proto::PodSandboxStatus {
        id: "sb-1".into(),
        metadata: Some(proto::PodSandboxMetadata {
            name: "web".into(),
            ..Default::default()
        }),
        state: proto::PodSandboxState::SandboxNotready as i32,
        created_at: 42,
        ..Default::default()
    };
    let n = t::PodSandboxStatus::from(p);
    assert_eq!(n.id, "sb-1");
    assert_eq!(n.metadata.name, "web");
    assert_eq!(n.state, t::PodSandboxState::NotReady);
    assert_eq!(n.created_at, 42);
}

#[test]
fn container_config_to_proto_maps_command_envs_mounts() {
    let cfg = t::ContainerConfig {
        metadata: t::ContainerMetadata {
            name: "app".into(),
            attempt: 1,
        },
        image: t::ImageSpec {
            image: "nginx:1.27".into(),
        },
        command: vec!["/bin/sh".into()],
        args: vec!["-c".into(), "sleep 1".into()],
        envs: vec![t::KeyValue {
            key: "K".into(),
            value: "V".into(),
        }],
        mounts: vec![t::Mount {
            container_path: "/data".into(),
            host_path: "/host/data".into(),
            readonly: true,
        }],
        log_path: "app/0.log".into(),
        labels: btree(&[("io.kubernetes.container.name", "app")]),
    };

    let p = proto::ContainerConfig::from(cfg);
    assert_eq!(p.metadata.expect("md").name, "app");
    assert_eq!(p.metadata.expect("md").attempt, 1);
    assert_eq!(p.image.expect("image").image, "nginx:1.27");
    assert_eq!(p.command, vec!["/bin/sh".to_owned()]);
    assert_eq!(p.args, vec!["-c".to_owned(), "sleep 1".to_owned()]);
    assert_eq!(p.envs.len(), 1);
    assert_eq!(p.envs[0].key, "K");
    assert_eq!(p.envs[0].value, "V");
    assert_eq!(p.mounts.len(), 1);
    assert_eq!(p.mounts[0].container_path, "/data");
    assert_eq!(p.mounts[0].host_path, "/host/data");
    assert!(p.mounts[0].readonly);
    assert_eq!(p.log_path, "app/0.log");
}

#[test]
fn container_state_enum_maps_all_four() {
    for (n, expect) in [
        (t::ContainerState::Created, proto::ContainerState::ContainerCreated),
        (t::ContainerState::Running, proto::ContainerState::ContainerRunning),
        (t::ContainerState::Exited, proto::ContainerState::ContainerExited),
        (t::ContainerState::Unknown, proto::ContainerState::ContainerUnknown),
    ] {
        assert_eq!(proto::ContainerState::from(n) as i32, expect as i32);
        assert_eq!(t::ContainerState::from(expect), n);
    }
}

#[test]
fn proto_container_to_native() {
    let p = proto::Container {
        id: "c-1".into(),
        pod_sandbox_id: "sb-1".into(),
        metadata: Some(proto::ContainerMetadata {
            name: "app".into(),
            attempt: 0,
        }),
        image: Some(proto::ImageSpec {
            image: "nginx:1.27".into(),
            ..Default::default()
        }),
        state: proto::ContainerState::ContainerRunning as i32,
        created_at: 7,
        labels: [("k".to_owned(), "v".to_owned())].into_iter().collect(),
        ..Default::default()
    };
    let n = t::Container::from(p);
    assert_eq!(n.id, "c-1");
    assert_eq!(n.pod_sandbox_id, "sb-1");
    assert_eq!(n.metadata.name, "app");
    assert_eq!(n.image.image, "nginx:1.27");
    assert_eq!(n.state, t::ContainerState::Running);
    assert_eq!(n.created_at, 7);
    assert_eq!(n.labels.get("k").map(String::as_str), Some("v"));
}

#[test]
fn proto_container_status_to_native_preserves_exit_info() {
    let p = proto::ContainerStatus {
        id: "c-1".into(),
        metadata: Some(proto::ContainerMetadata {
            name: "app".into(),
            attempt: 0,
        }),
        state: proto::ContainerState::ContainerExited as i32,
        created_at: 1,
        started_at: 2,
        finished_at: 3,
        exit_code: 137,
        image: Some(proto::ImageSpec {
            image: "nginx:1.27".into(),
            ..Default::default()
        }),
        reason: "OOMKilled".into(),
        message: "out of memory".into(),
        ..Default::default()
    };
    let n = t::ContainerStatus::from(p);
    assert_eq!(n.id, "c-1");
    assert_eq!(n.metadata.name, "app");
    assert_eq!(n.state, t::ContainerState::Exited);
    assert_eq!(n.created_at, 1);
    assert_eq!(n.started_at, 2);
    assert_eq!(n.finished_at, 3);
    assert_eq!(n.exit_code, 137);
    assert_eq!(n.image.image, "nginx:1.27");
    assert_eq!(n.reason, "OOMKilled");
    assert_eq!(n.message, "out of memory");
}

#[test]
fn proto_image_to_native_renames_size() {
    let p = proto::Image {
        id: "sha256:abc".into(),
        repo_tags: vec!["nginx:1.27".into()],
        size: 12_345,
        ..Default::default()
    };
    let n = t::Image::from(p);
    assert_eq!(n.id, "sha256:abc");
    assert_eq!(n.repo_tags, vec!["nginx:1.27".to_owned()]);
    assert_eq!(n.size_bytes, 12_345);
}

#[test]
fn image_spec_to_proto() {
    let p = proto::ImageSpec::from(t::ImageSpec {
        image: "redis:7".into(),
    });
    assert_eq!(p.image, "redis:7");
}

#[test]
fn pod_sandbox_filter_to_proto_carries_state_value() {
    let f = t::PodSandboxFilter {
        id: Some("sb-1".into()),
        state: Some(t::PodSandboxState::Ready),
    };
    let p = proto::PodSandboxFilter::from(f);
    assert_eq!(p.id, "sb-1");
    assert_eq!(
        p.state.expect("state").state,
        proto::PodSandboxState::SandboxReady as i32
    );

    // No state filter -> proto state stays None.
    let none = proto::PodSandboxFilter::from(t::PodSandboxFilter::default());
    assert!(none.state.is_none());
}

#[test]
fn container_filter_to_proto_carries_sandbox_and_state() {
    let f = t::ContainerFilter {
        id: Some("c-1".into()),
        pod_sandbox_id: Some("sb-1".into()),
        state: Some(t::ContainerState::Running),
    };
    let p = proto::ContainerFilter::from(f);
    assert_eq!(p.id, "c-1");
    assert_eq!(p.pod_sandbox_id, "sb-1");
    assert_eq!(
        p.state.expect("state").state,
        proto::ContainerState::ContainerRunning as i32
    );
}
