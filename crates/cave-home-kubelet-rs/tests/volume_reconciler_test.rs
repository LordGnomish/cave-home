// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::volume::reconciler::Reconciler`
//! plus DSW/ASW caches.
//!
//! Hand-port targets:
//! - upstream_test: pkg/kubelet/volumemanager/cache/desired_state_of_world_test.go::TestAddPodToVolume
//! - upstream_test: pkg/kubelet/volumemanager/cache/desired_state_of_world_test.go::TestDeletePodFromVolume
//! - upstream_test: pkg/kubelet/volumemanager/cache/actual_state_of_world_test.go::TestMarkVolumeAsAttached
//! - upstream_test: pkg/kubelet/volumemanager/reconciler/reconciler_test.go::TestReconcileWithUnmountDevice

use std::sync::Arc;

use cave_home_kubelet_rs::api::{EmptyDirVolumeSource, PodUid, Volume, VolumeSource};
use cave_home_kubelet_rs::volume::emptydir::EmptyDirPlugin;
use cave_home_kubelet_rs::volume::{
    ActualStateOfWorld, DesiredStateOfWorld, Reconciler, VolumePlugin,
};

fn empty_dir(name: &str) -> Volume {
    Volume {
        name: name.into(),
        source: VolumeSource::EmptyDir(EmptyDirVolumeSource::default()),
    }
}

#[test]
fn dsw_add_remove_pod_round_trips() {
    let dsw = DesiredStateOfWorld::new();
    assert!(dsw.is_empty());
    dsw.add_pod(PodUid::new("u"), vec![empty_dir("a"), empty_dir("b")]);
    assert!(dsw.has_pod(&PodUid::new("u")));
    assert_eq!(dsw.len(), 1);
    assert_eq!(dsw.snapshot().len(), 2);
    dsw.remove_pod(&PodUid::new("u"));
    assert!(dsw.is_empty());
}

#[test]
fn asw_mark_and_unmark_volumes() {
    let asw = ActualStateOfWorld::new();
    asw.add_mounted(cave_home_kubelet_rs::volume::actual::MountedVolume {
        pod_uid: PodUid::new("u"),
        volume_name: "data".into(),
        host_path: std::path::PathBuf::from("/tmp/x"),
    });
    assert!(asw.is_mounted(&PodUid::new("u"), "data"));
    assert_eq!(asw.len(), 1);
    asw.remove_mounted(&PodUid::new("u"), "data");
    assert!(asw.is_empty());
}

#[tokio::test]
async fn reconciler_mounts_desired_emptydir_volumes() {
    let tmp = tempfile::tempdir().unwrap();
    let dsw = Arc::new(DesiredStateOfWorld::new());
    let asw = Arc::new(ActualStateOfWorld::new());
    let plugin: Arc<dyn VolumePlugin> = Arc::new(EmptyDirPlugin::new(tmp.path()));
    let r = Reconciler::new(vec![plugin], dsw.clone(), asw.clone());

    dsw.add_pod(PodUid::new("u"), vec![empty_dir("data")]);
    r.reconcile_once().await.unwrap();
    assert!(asw.is_mounted(&PodUid::new("u"), "data"));
    let p = asw.get_host_path(&PodUid::new("u"), "data").unwrap();
    assert!(p.exists());
}

#[tokio::test]
async fn reconciler_unmounts_volumes_no_longer_desired() {
    let tmp = tempfile::tempdir().unwrap();
    let dsw = Arc::new(DesiredStateOfWorld::new());
    let asw = Arc::new(ActualStateOfWorld::new());
    let plugin: Arc<dyn VolumePlugin> = Arc::new(EmptyDirPlugin::new(tmp.path()));
    let r = Reconciler::new(vec![plugin], dsw.clone(), asw.clone());

    dsw.add_pod(PodUid::new("u"), vec![empty_dir("data")]);
    r.reconcile_once().await.unwrap();
    assert!(asw.is_mounted(&PodUid::new("u"), "data"));
    let p = asw.get_host_path(&PodUid::new("u"), "data").unwrap();
    assert!(p.exists());

    dsw.remove_pod(&PodUid::new("u"));
    r.reconcile_once().await.unwrap();
    assert!(!asw.is_mounted(&PodUid::new("u"), "data"));
    assert!(!p.exists(), "tear-down should remove the host path");
}

#[tokio::test]
async fn reconciler_skips_already_mounted_volumes() {
    let tmp = tempfile::tempdir().unwrap();
    let dsw = Arc::new(DesiredStateOfWorld::new());
    let asw = Arc::new(ActualStateOfWorld::new());
    let plugin: Arc<dyn VolumePlugin> = Arc::new(EmptyDirPlugin::new(tmp.path()));
    let r = Reconciler::new(vec![plugin], dsw.clone(), asw.clone());

    dsw.add_pod(PodUid::new("u"), vec![empty_dir("data")]);
    r.reconcile_once().await.unwrap();
    let p1 = asw.get_host_path(&PodUid::new("u"), "data").unwrap();
    r.reconcile_once().await.unwrap();
    let p2 = asw.get_host_path(&PodUid::new("u"), "data").unwrap();
    assert_eq!(p1, p2);
}

#[tokio::test]
async fn reconciler_handles_multiple_pods() {
    let tmp = tempfile::tempdir().unwrap();
    let dsw = Arc::new(DesiredStateOfWorld::new());
    let asw = Arc::new(ActualStateOfWorld::new());
    let plugin: Arc<dyn VolumePlugin> = Arc::new(EmptyDirPlugin::new(tmp.path()));
    let r = Reconciler::new(vec![plugin], dsw.clone(), asw.clone());

    dsw.add_pod(PodUid::new("u1"), vec![empty_dir("a")]);
    dsw.add_pod(PodUid::new("u2"), vec![empty_dir("a"), empty_dir("b")]);
    r.reconcile_once().await.unwrap();
    assert_eq!(asw.len(), 3);
}
