// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::api`.
//!
//! Hand-port targets (`pkg/kubelet`):
//! - upstream_test: `staging/src/k8s.io/api/core/v1/types_test.go::TestPodUID`
//! - upstream_test: `staging/src/k8s.io/api/core/v1/types_test.go::TestVolumeSource`

use cave_home_kubelet_rs::api::{
    Container, ContainerState, EmptyDirVolumeSource, HostPathType, HostPathVolumeSource,
    ObjectMeta, Pod, PodPhase, PodSpec, PodUid, RestartPolicy, Volume, VolumeMount, VolumeSource,
};

#[test]
fn pod_uid_round_trips_through_string() {
    let uid = PodUid::new("11112222-3333-4444-5555-666677778888");
    assert_eq!(uid.as_str(), "11112222-3333-4444-5555-666677778888");
}

#[test]
fn pod_full_name_combines_namespace_and_name() {
    let pod = Pod {
        metadata: ObjectMeta {
            name: "nginx".into(),
            namespace: "default".into(),
            uid: PodUid::new("abc"),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(pod.full_name(), "default/nginx");
}

#[test]
fn host_path_type_enumerates_all_kubernetes_variants() {
    // Mirrors the eight values defined in `core/v1/types.go::HostPathType`.
    let variants = [
        HostPathType::Unset,
        HostPathType::DirectoryOrCreate,
        HostPathType::Directory,
        HostPathType::FileOrCreate,
        HostPathType::File,
        HostPathType::Socket,
        HostPathType::CharDevice,
        HostPathType::BlockDevice,
    ];
    assert_eq!(variants.len(), 8);
    assert_eq!(HostPathType::default(), HostPathType::Unset);
}

#[test]
fn restart_policy_default_matches_kubernetes_default() {
    // `core/v1/types.go::PodSpec.RestartPolicy` defaults to "Always".
    assert_eq!(RestartPolicy::default(), RestartPolicy::Always);
}

#[test]
fn pod_phase_default_is_pending() {
    assert_eq!(PodPhase::default(), PodPhase::Pending);
}

#[test]
fn container_state_default_is_waiting() {
    assert!(matches!(
        ContainerState::default(),
        ContainerState::Waiting(_)
    ));
}

#[test]
fn volume_source_distinguishes_emptydir_from_hostpath() {
    let v1 = VolumeSource::EmptyDir(EmptyDirVolumeSource::default());
    let v2 = VolumeSource::HostPath(HostPathVolumeSource {
        path: "/data".into(),
        host_path_type: HostPathType::Directory,
    });
    assert_ne!(v1, v2);
}

#[test]
fn pod_spec_defaults_to_empty_containers_and_volumes() {
    let spec = PodSpec::default();
    assert!(spec.containers.is_empty());
    assert!(spec.volumes.is_empty());
}

#[test]
fn container_can_declare_volume_mounts() {
    let c = Container {
        name: "main".into(),
        image: "nginx:latest".into(),
        volume_mounts: vec![VolumeMount {
            name: "data".into(),
            mount_path: "/data".into(),
            read_only: false,
        }],
        ..Default::default()
    };
    assert_eq!(c.volume_mounts.len(), 1);
    assert_eq!(c.volume_mounts[0].mount_path, "/data");
}

#[test]
fn pod_full_name_with_volume_mount() {
    let pod = Pod {
        metadata: ObjectMeta {
            name: "n".into(),
            namespace: "ns".into(),
            uid: PodUid::new("u"),
            ..Default::default()
        },
        spec: PodSpec {
            containers: vec![Container {
                name: "c".into(),
                ..Default::default()
            }],
            volumes: vec![Volume {
                name: "v".into(),
                source: VolumeSource::EmptyDir(EmptyDirVolumeSource::default()),
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(pod.full_name(), "ns/n");
    assert_eq!(pod.spec.containers.len(), 1);
    assert_eq!(pod.spec.volumes.len(), 1);
}
