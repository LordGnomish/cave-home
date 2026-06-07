// SPDX-License-Identifier: Apache-2.0
//! Snapshotter mount assembly — the overlayfs and native backends' decision
//! for *how* a snapshot's filesystem is mounted.
//!
//! Behavioural reimplementation of the documented containerd snapshotter
//! `mounts()` decision (`snapshots/overlay/overlay.go` and
//! `snapshots/native/native.go`): given a [`Snapshot`] (its id, its
//! [`Kind`], and its ordered parent ids), compute the list of [`Mount`]s the
//! caller must apply — the bind-vs-overlay choice and, for overlay, the
//! `lowerdir` / `upperdir` / `workdir` option assembly.
//!
//! This is the *decision* only — pure path/string logic. The kernel-side work
//! that consumes these mounts (the `mount(2)` syscalls, layer unpack/diff to
//! disk, the blob content store) is filesystem/syscall-bound and remains
//! deferred (see `parity.manifest.toml`). The mounts produced here are exactly
//! what containerd hands to its mount layer, unchanged.
//!
//! Spec sources:
//!   * containerd `snapshots/overlay/overlay.go` `mounts()` — the no-parent
//!     bind fallback (overlay needs ≥2 dirs), the single-parent read-only bind,
//!     the active `workdir`+`upperdir` pair, the `lowerdir` parent join (most
//!     recent parent first), and the `index=off` / `userxattr` option prefixes.
//!   * containerd `snapshots/native/native.go` `mounts()` — a single bind
//!     mount of the snapshot's own dir (active / no parents) or the immediate
//!     parent's dir (read-only view), with the `ro`/`rw` + `rbind` flags.
//!   * containerd `snapshots/snapshots.go` `Kind` — `Active` (writable, has an
//!     upperdir) vs `View` (read-only); only these two kinds are ever mounted.

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/var/lib/overlay";
    const NROOT: &str = "/var/lib/native";

    fn snap(id: &str, kind: Kind, parents: &[&str]) -> Snapshot {
        Snapshot {
            id: id.to_string(),
            kind,
            parent_ids: parents.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    // -- overlay path helpers ------------------------------------------------

    #[test]
    fn overlay_path_helpers_layout() {
        let o = OverlaySnapshotter::new(ROOT);
        assert_eq!(o.snapshot_dir("5"), "/var/lib/overlay/snapshots/5");
        assert_eq!(o.upper_path("5"), "/var/lib/overlay/snapshots/5/fs");
        assert_eq!(o.work_path("5"), "/var/lib/overlay/snapshots/5/work");
    }

    #[test]
    fn overlay_root_trailing_slash_is_normalised() {
        let o = OverlaySnapshotter::new("/var/lib/overlay/");
        assert_eq!(o.upper_path("5"), "/var/lib/overlay/snapshots/5/fs");
    }

    // -- overlay: no parents -> bind (overlay needs >= 2 dirs) ---------------

    #[test]
    fn overlay_no_parent_active_is_rw_bind_of_own_upper() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("5", Kind::Active, &[]));
        assert_eq!(mounts.len(), 1);
        let m = &mounts[0];
        assert_eq!(m.mount_type, "bind");
        assert_eq!(m.source, "/var/lib/overlay/snapshots/5/fs");
        assert_eq!(m.options, vec!["rw".to_string(), "rbind".to_string()]);
    }

    #[test]
    fn overlay_no_parent_view_is_ro_bind() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("5", Kind::View, &[]));
        assert_eq!(mounts[0].mount_type, "bind");
        assert_eq!(mounts[0].options, vec!["ro".to_string(), "rbind".to_string()]);
    }

    // -- overlay: single parent ---------------------------------------------

    #[test]
    fn overlay_single_parent_active_is_overlay_with_workdir_upper_lower() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("5", Kind::Active, &["4"]));
        assert_eq!(mounts.len(), 1);
        let m = &mounts[0];
        assert_eq!(m.mount_type, "overlay");
        assert_eq!(m.source, "overlay");
        assert_eq!(
            m.options,
            vec![
                "workdir=/var/lib/overlay/snapshots/5/work".to_string(),
                "upperdir=/var/lib/overlay/snapshots/5/fs".to_string(),
                "lowerdir=/var/lib/overlay/snapshots/4/fs".to_string(),
            ]
        );
    }

    #[test]
    fn overlay_single_parent_view_is_ro_bind_of_parent_upper() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("5", Kind::View, &["4"]));
        assert_eq!(mounts.len(), 1);
        let m = &mounts[0];
        assert_eq!(m.mount_type, "bind");
        assert_eq!(m.source, "/var/lib/overlay/snapshots/4/fs");
        assert_eq!(m.options, vec!["ro".to_string(), "rbind".to_string()]);
    }

    // -- overlay: multiple parents ------------------------------------------

    #[test]
    fn overlay_multi_parent_active_options_order_and_lowerdir_join() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("9", Kind::Active, &["8", "7", "6"]));
        let m = &mounts[0];
        assert_eq!(m.mount_type, "overlay");
        assert_eq!(
            m.options,
            vec![
                "workdir=/var/lib/overlay/snapshots/9/work".to_string(),
                "upperdir=/var/lib/overlay/snapshots/9/fs".to_string(),
                "lowerdir=/var/lib/overlay/snapshots/8/fs:/var/lib/overlay/snapshots/7/fs:/var/lib/overlay/snapshots/6/fs".to_string(),
            ]
        );
    }

    #[test]
    fn overlay_multi_parent_view_is_lowerdir_only_overlay() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("9", Kind::View, &["8", "7"]));
        let m = &mounts[0];
        assert_eq!(m.mount_type, "overlay");
        assert_eq!(
            m.options,
            vec![
                "lowerdir=/var/lib/overlay/snapshots/8/fs:/var/lib/overlay/snapshots/7/fs"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn overlay_lowerdir_keeps_most_recent_parent_first() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("3", Kind::View, &["2", "1", "0"]));
        let lower = mounts[0].options.last().expect("lowerdir option");
        assert_eq!(
            lower,
            "lowerdir=/var/lib/overlay/snapshots/2/fs:/var/lib/overlay/snapshots/1/fs:/var/lib/overlay/snapshots/0/fs"
        );
    }

    // -- overlay: index=off / userxattr prefixes ----------------------------

    #[test]
    fn overlay_index_off_and_userxattr_prefix_options_in_order() {
        let o = OverlaySnapshotter::with_options(ROOT, true, true);
        let mounts = o.mounts(&snap("9", Kind::Active, &["8", "7"]));
        let m = &mounts[0];
        assert_eq!(
            m.options,
            vec![
                "index=off".to_string(),
                "userxattr".to_string(),
                "workdir=/var/lib/overlay/snapshots/9/work".to_string(),
                "upperdir=/var/lib/overlay/snapshots/9/fs".to_string(),
                "lowerdir=/var/lib/overlay/snapshots/8/fs:/var/lib/overlay/snapshots/7/fs"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn overlay_options_default_to_off() {
        let o = OverlaySnapshotter::new(ROOT);
        let mounts = o.mounts(&snap("9", Kind::View, &["8", "7"]));
        assert!(!mounts[0].options.iter().any(|opt| opt == "index=off"));
        assert!(!mounts[0].options.iter().any(|opt| opt == "userxattr"));
    }

    // -- native --------------------------------------------------------------

    #[test]
    fn native_snapshot_dir_layout() {
        let n = NativeSnapshotter::new(NROOT);
        assert_eq!(n.snapshot_dir("7"), "/var/lib/native/snapshots/7");
    }

    #[test]
    fn native_no_parent_active_is_rw_bind_of_own_dir() {
        let n = NativeSnapshotter::new(NROOT);
        let mounts = n.mounts(&snap("7", Kind::Active, &[]));
        assert_eq!(mounts.len(), 1);
        let m = &mounts[0];
        assert_eq!(m.mount_type, "bind");
        assert_eq!(m.source, "/var/lib/native/snapshots/7");
        assert_eq!(m.options, vec!["rw".to_string(), "rbind".to_string()]);
    }

    #[test]
    fn native_no_parent_view_is_ro_bind() {
        let n = NativeSnapshotter::new(NROOT);
        let mounts = n.mounts(&snap("7", Kind::View, &[]));
        assert_eq!(mounts[0].options, vec!["ro".to_string(), "rbind".to_string()]);
        assert_eq!(mounts[0].source, "/var/lib/native/snapshots/7");
    }

    #[test]
    fn native_active_with_parents_still_uses_own_dir() {
        let n = NativeSnapshotter::new(NROOT);
        let mounts = n.mounts(&snap("7", Kind::Active, &["6", "5"]));
        let m = &mounts[0];
        assert_eq!(m.source, "/var/lib/native/snapshots/7");
        assert_eq!(m.options, vec!["rw".to_string(), "rbind".to_string()]);
    }

    #[test]
    fn native_view_with_parents_uses_immediate_parent_dir() {
        let n = NativeSnapshotter::new(NROOT);
        let mounts = n.mounts(&snap("7", Kind::View, &["6", "5"]));
        let m = &mounts[0];
        assert_eq!(m.mount_type, "bind");
        assert_eq!(m.source, "/var/lib/native/snapshots/6");
        assert_eq!(m.options, vec!["ro".to_string(), "rbind".to_string()]);
    }

    // -- Kind helper ---------------------------------------------------------

    #[test]
    fn kind_writable_only_for_active() {
        assert!(Kind::Active.is_writable());
        assert!(!Kind::View.is_writable());
    }
}
