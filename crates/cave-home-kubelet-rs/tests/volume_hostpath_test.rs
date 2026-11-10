// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::volume::hostpath::HostPathPlugin`.
//!
//! Hand-port targets — `pkg/volume/hostpath/host_path_test.go`:
//! - `upstream_test`: `TestCanSupport`
//! - `upstream_test`: `TestPlugin`
//! - `upstream_test`: `TestSetUp`
//! - `upstream_test`: `TestTearDown`
//! - `upstream_test`: `TestPathDoesNotExist`
//! - `upstream_test`: `TestNotADirectory`

use cave_home_kubelet_rs::api::{
    EmptyDirVolumeSource, HostPathType, HostPathVolumeSource, PodUid, Volume, VolumeSource,
};
use cave_home_kubelet_rs::volume::hostpath::HostPathPlugin;
use cave_home_kubelet_rs::volume::plugin::{VolumeError, VolumePlugin};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn host_volume(path: &str, t: HostPathType) -> Volume {
    Volume {
        name: "host".into(),
        source: VolumeSource::HostPath(HostPathVolumeSource {
            path: path.into(),
            host_path_type: t,
        }),
    }
}

#[tokio::test]
async fn name_is_kubernetes_io_host_path() {
    assert_eq!(HostPathPlugin::new().name(), "kubernetes.io/host-path");
}

#[tokio::test]
async fn can_support_host_path_only() {
    let p = HostPathPlugin::new();
    assert!(p.can_support(&host_volume("/tmp", HostPathType::Directory)));
    let ed = Volume {
        name: "x".into(),
        source: VolumeSource::EmptyDir(EmptyDirVolumeSource::default()),
    };
    assert!(!p.can_support(&ed));
}

#[tokio::test]
async fn set_up_returns_the_host_path() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let path_str = tmp.path().to_string_lossy().to_string();
    let p = HostPathPlugin::new();
    let v = host_volume(&path_str, HostPathType::Directory);
    let got = p.set_up(&PodUid::new("u"), &v).await?;
    assert_eq!(got, tmp.path());
    Ok(())
}

#[tokio::test]
async fn set_up_directory_or_create_creates_missing_directory() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("new-dir");
    let p = HostPathPlugin::new();
    let v = host_volume(
        target.to_str().ok_or("non-UTF-8 temp path")?,
        HostPathType::DirectoryOrCreate,
    );
    p.set_up(&PodUid::new("u"), &v).await?;
    assert!(target.exists());
    assert!(target.is_dir());
    Ok(())
}

#[tokio::test]
async fn set_up_directory_rejects_missing_path() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("missing");
    let p = HostPathPlugin::new();
    let v = host_volume(
        target.to_str().ok_or("non-UTF-8 temp path")?,
        HostPathType::Directory,
    );
    let err = p
        .set_up(&PodUid::new("u"), &v)
        .await
        .err()
        .ok_or("expected an error")?;
    assert!(matches!(err, VolumeError::InvalidHostPath(_)));
    Ok(())
}

#[tokio::test]
async fn set_up_directory_rejects_when_path_is_a_file() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("a-file");
    std::fs::write(&target, b"hi")?;
    let p = HostPathPlugin::new();
    let v = host_volume(
        target.to_str().ok_or("non-UTF-8 temp path")?,
        HostPathType::Directory,
    );
    let err = p
        .set_up(&PodUid::new("u"), &v)
        .await
        .err()
        .ok_or("expected an error")?;
    assert!(matches!(err, VolumeError::InvalidHostPath(_)));
    Ok(())
}

#[tokio::test]
async fn set_up_file_or_create_touches_missing_file() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("new-file");
    let p = HostPathPlugin::new();
    let v = host_volume(
        target.to_str().ok_or("non-UTF-8 temp path")?,
        HostPathType::FileOrCreate,
    );
    p.set_up(&PodUid::new("u"), &v).await?;
    assert!(target.exists());
    assert!(target.is_file());
    Ok(())
}

#[tokio::test]
async fn tear_down_is_a_no_op_for_host_path() -> TestResult {
    // HostPath is owned by the host; tear-down must NOT touch it.
    let tmp = tempfile::tempdir()?;
    let p = HostPathPlugin::new();
    let v = host_volume(
        tmp.path().to_str().ok_or("non-UTF-8 temp path")?,
        HostPathType::Directory,
    );
    p.tear_down(&PodUid::new("u"), &v).await?;
    assert!(tmp.path().exists());
    Ok(())
}
