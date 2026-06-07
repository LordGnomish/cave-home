// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::volume::emptydir::EmptyDirPlugin`.
//!
//! Hand-port targets — `pkg/volume/emptydir/empty_dir_test.go`:
//! - upstream_test: TestPluginEmptyRootContext
//! - upstream_test: TestPluginTmpfs
//! - upstream_test: TestPluginHugetlbfs
//! - upstream_test: TestMetrics
//! - upstream_test: TestSetUp
//! - upstream_test: TestTearDown
//! - upstream_test: TestCanSupport
//! - upstream_test: TestGetAccessModes

use std::path::PathBuf;

use cave_home_kubelet_rs::api::{
    EmptyDirVolumeSource, HostPathType, HostPathVolumeSource, PodUid, Volume, VolumeSource,
};
use cave_home_kubelet_rs::volume::emptydir::EmptyDirPlugin;
use cave_home_kubelet_rs::volume::plugin::VolumePlugin;

fn empty_dir(name: &str) -> Volume {
    Volume {
        name: name.into(),
        source: VolumeSource::EmptyDir(EmptyDirVolumeSource::default()),
    }
}

#[tokio::test]
async fn name_is_kubernetes_io_empty_dir() {
    let p = EmptyDirPlugin::default();
    assert_eq!(p.name(), "kubernetes.io/empty-dir");
}

#[tokio::test]
async fn can_support_empty_dir_only() {
    let p = EmptyDirPlugin::default();
    assert!(p.can_support(&empty_dir("data")));
    let host = Volume {
        name: "host".into(),
        source: VolumeSource::HostPath(HostPathVolumeSource {
            path: "/tmp".into(),
            host_path_type: HostPathType::Directory,
        }),
    };
    assert!(!p.can_support(&host));
}

#[tokio::test]
async fn host_path_layout_matches_kubelet_default() {
    let p = EmptyDirPlugin::default();
    let path = p.host_path(&PodUid::new("uid-1"), "data");
    assert_eq!(
        path,
        PathBuf::from("/var/lib/cave-home-kubelet/pods/uid-1/volumes/kubernetes.io~empty-dir/data")
    );
}

#[tokio::test]
async fn set_up_creates_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let p = EmptyDirPlugin::new(tmp.path());
    let path = p
        .set_up(&PodUid::new("uid-1"), &empty_dir("data"))
        .await
        .unwrap();
    assert!(path.exists());
    assert!(path.is_dir());
    assert!(
        path.to_string_lossy()
            .contains("kubernetes.io~empty-dir/data")
    );
}

#[tokio::test]
async fn set_up_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let p = EmptyDirPlugin::new(tmp.path());
    let p1 = p
        .set_up(&PodUid::new("uid-1"), &empty_dir("data"))
        .await
        .unwrap();
    let p2 = p
        .set_up(&PodUid::new("uid-1"), &empty_dir("data"))
        .await
        .unwrap();
    assert_eq!(p1, p2);
}

#[tokio::test]
async fn tear_down_removes_the_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let p = EmptyDirPlugin::new(tmp.path());
    let path = p
        .set_up(&PodUid::new("uid-1"), &empty_dir("data"))
        .await
        .unwrap();
    assert!(path.exists());
    p.tear_down(&PodUid::new("uid-1"), &empty_dir("data"))
        .await
        .unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn tear_down_missing_directory_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let p = EmptyDirPlugin::new(tmp.path());
    p.tear_down(&PodUid::new("never-set-up"), &empty_dir("data"))
        .await
        .unwrap();
}

#[tokio::test]
async fn each_pod_uid_gets_its_own_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let p = EmptyDirPlugin::new(tmp.path());
    let a = p
        .set_up(&PodUid::new("uid-a"), &empty_dir("data"))
        .await
        .unwrap();
    let b = p
        .set_up(&PodUid::new("uid-b"), &empty_dir("data"))
        .await
        .unwrap();
    assert_ne!(a, b);
    assert!(a.exists());
    assert!(b.exists());
}
