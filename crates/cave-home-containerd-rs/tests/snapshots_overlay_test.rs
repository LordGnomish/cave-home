// SPDX-License-Identifier: Apache-2.0
//! Overlay snapshotter tests — port of upstream's
//! `plugins/snapshots/overlay/overlay_test.go` happy paths plus the
//! lowerdir/upperdir/workdir formatting from
//! `mounts()` (overlay.go:552-615).

use cave_home_containerd_rs::snapshots::{Kind, SnapshotError, Snapshotter};
use tempfile::TempDir;

async fn fixture() -> (TempDir, Snapshotter) {
    let td = tempfile::tempdir().unwrap();
    let s = Snapshotter::open(td.path()).await.unwrap();
    (td, s)
}

#[tokio::test]
async fn test_open_creates_root_layout() {
    let (td, _s) = fixture().await;
    assert!(td.path().join("snapshots").is_dir());
}

#[tokio::test]
async fn test_prepare_creates_fs_and_work_dirs() {
    let (td, s) = fixture().await;
    let mounts = s.prepare("k1", None).await.unwrap();
    // Single bind mount when no parents (overlay.go:564-580).
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].mount_type, "bind");

    // The fs/ directory exists somewhere under <root>/snapshots/.
    let snaps = std::fs::read_dir(td.path().join("snapshots")).unwrap();
    let mut found = false;
    for e in snaps {
        let e = e.unwrap();
        if e.path().join("fs").is_dir() && e.path().join("work").is_dir() {
            found = true;
            break;
        }
    }
    assert!(found, "expected an active snapshot dir with fs/ and work/");
}

#[tokio::test]
async fn test_prepare_duplicate_key_errors() {
    let (_td, s) = fixture().await;
    s.prepare("dup", None).await.unwrap();
    let err = s.prepare("dup", None).await.unwrap_err();
    assert!(matches!(err, SnapshotError::AlreadyExists(_)));
}

#[tokio::test]
async fn test_prepare_with_unknown_parent_errors() {
    let (_td, s) = fixture().await;
    let err = s.prepare("child", Some("nope")).await.unwrap_err();
    assert!(matches!(err, SnapshotError::ParentNotFound(_)));
}

#[tokio::test]
async fn test_commit_promotes_active_to_committed() {
    let (_td, s) = fixture().await;
    s.prepare("k1", None).await.unwrap();
    s.commit("base", "k1").await.unwrap();
    let info = s.stat("base").await.unwrap();
    assert_eq!(info.kind, Kind::Committed);
    // The active key is gone.
    assert!(matches!(s.stat("k1").await.unwrap_err(), SnapshotError::NotFound(_)));
}

#[tokio::test]
async fn test_mounts_overlay_string_for_chained_snapshot() {
    let (_td, s) = fixture().await;
    s.prepare("base-key", None).await.unwrap();
    s.commit("base", "base-key").await.unwrap();
    let mounts = s.prepare("child", Some("base")).await.unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].mount_type, "overlay");
    assert_eq!(mounts[0].source, "overlay");
    let opts = mounts[0].options.join(",");
    assert!(opts.contains("lowerdir="), "lowerdir missing: {opts}");
    assert!(opts.contains("upperdir="), "upperdir missing: {opts}");
    assert!(opts.contains("workdir="), "workdir missing: {opts}");
}

#[tokio::test]
async fn test_view_returns_ro_bind_when_single_parent() {
    // overlay.go:588-599: single-parent View → ro,rbind on parent's upper
    let (_td, s) = fixture().await;
    s.prepare("base-key", None).await.unwrap();
    s.commit("base", "base-key").await.unwrap();
    let mounts = s.view("v1", Some("base")).await.unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].mount_type, "bind");
    assert!(mounts[0].options.iter().any(|o| o == "ro"));
    assert!(mounts[0].options.iter().any(|o| o == "rbind"));
}

#[tokio::test]
async fn test_remove_then_stat_is_not_found() {
    let (_td, s) = fixture().await;
    s.prepare("k", None).await.unwrap();
    s.remove("k").await.unwrap();
    assert!(matches!(s.stat("k").await.unwrap_err(), SnapshotError::NotFound(_)));
}

#[tokio::test]
async fn test_walk_visits_all_snapshots() {
    let (_td, s) = fixture().await;
    s.prepare("a", None).await.unwrap();
    s.prepare("b", None).await.unwrap();
    s.prepare("c", None).await.unwrap();
    let mut count = 0usize;
    s.walk(|_| count += 1).await.unwrap();
    assert_eq!(count, 3);
}
