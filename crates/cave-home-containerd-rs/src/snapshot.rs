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

/// The kind of a snapshot at mount time.
///
/// containerd's `snapshots.Kind` also has `Unknown` and `Committed`, but only
/// `Active` (a writable working snapshot with its own upperdir) and `View` (a
/// read-only view of a committed layer chain) are ever passed to `mounts()`,
/// so those are the only two modelled here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A writable working snapshot — gets an upperdir/workdir, mounted `rw`.
    Active,
    /// A read-only view of a committed layer chain — mounted `ro`.
    View,
}

impl Kind {
    /// True only for [`Kind::Active`]; a [`Kind::View`] is read-only.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        matches!(self, Self::Active)
    }

    /// The mount read/write flag (`"rw"` for active, `"ro"` for a view).
    const fn rw_flag(self) -> &'static str {
        match self {
            Self::Active => "rw",
            Self::View => "ro",
        }
    }
}

/// A snapshot as the storage layer presents it to `mounts()`: an id, a
/// [`Kind`], and the ids of its parent snapshots ordered **most-recent
/// parent first** (`parent_ids[0]` is the immediate parent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The snapshot's own storage id.
    pub id: String,
    /// Writable (`Active`) or read-only (`View`).
    pub kind: Kind,
    /// Parent snapshot ids, immediate parent first (empty for a base layer).
    pub parent_ids: Vec<String>,
}

/// A mount the caller must apply to materialise a snapshot's filesystem — the
/// shape containerd hands to its mount layer (`mount.Mount`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// The filesystem type: `"bind"` or `"overlay"`.
    pub mount_type: String,
    /// The mount source: a path for a bind mount, the literal `"overlay"` for
    /// an overlay mount.
    pub source: String,
    /// The mount options, in the exact order containerd assembles them.
    pub options: Vec<String>,
}

impl Mount {
    /// A bind mount of `source` with the read/write flag for `kind` + `rbind`.
    fn bind(source: String, kind: Kind) -> Self {
        Self {
            mount_type: "bind".to_string(),
            source,
            options: vec![kind.rw_flag().to_string(), "rbind".to_string()],
        }
    }
}

/// Joins a snapshotter root with the `snapshots/<id>` path component, tolerating
/// a trailing slash on the root (containerd's `filepath.Join` semantics).
fn snapshot_dir(root: &str, id: &str) -> String {
    format!("{}/snapshots/{}", root.trim_end_matches('/'), id)
}

/// The overlayfs snapshotter's mount-assembly decision.
///
/// Mirrors `snapshots/overlay/overlay.go` `mounts()`: each snapshot id owns a
/// directory `<root>/snapshots/<id>` with an `fs` upperdir and a `work`
/// workdir. The mount produced depends on the parent count and [`Kind`].
#[derive(Debug, Clone)]
pub struct OverlaySnapshotter {
    root: String,
    index_off: bool,
    userxattr: bool,
}

impl OverlaySnapshotter {
    /// A snapshotter rooted at `root` with `index=off` and `userxattr` both
    /// disabled (containerd enables these from kernel capability detection,
    /// which is a runtime concern deferred here).
    #[must_use]
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            index_off: false,
            userxattr: false,
        }
    }

    /// A snapshotter with the `index=off` / `userxattr` overlay options set
    /// explicitly.
    #[must_use]
    pub fn with_options(root: impl Into<String>, index_off: bool, userxattr: bool) -> Self {
        Self {
            root: root.into(),
            index_off,
            userxattr,
        }
    }

    /// The directory owning snapshot `id`: `<root>/snapshots/<id>`.
    #[must_use]
    pub fn snapshot_dir(&self, id: &str) -> String {
        snapshot_dir(&self.root, id)
    }

    /// The upperdir (diff dir) for `id`: `<root>/snapshots/<id>/fs`.
    #[must_use]
    pub fn upper_path(&self, id: &str) -> String {
        format!("{}/fs", self.snapshot_dir(id))
    }

    /// The workdir for `id`: `<root>/snapshots/<id>/work`.
    #[must_use]
    pub fn work_path(&self, id: &str) -> String {
        format!("{}/work", self.snapshot_dir(id))
    }

    /// Computes the mount(s) to apply for `snapshot`.
    ///
    /// The decision, exactly as containerd assembles it:
    ///
    /// - **no parents** → a single bind mount of the snapshot's own upperdir
    ///   (overlay needs ≥2 dirs to be meaningful), `rw`/`ro` per [`Kind`];
    /// - **single parent, [`Kind::View`]** (no upperdir to stack) → a
    ///   read-only bind of the parent's upperdir;
    /// - **otherwise** → an `overlay` mount whose options are, in order:
    ///   `index=off`?, `userxattr`?, then for an active snapshot `workdir=` and
    ///   `upperdir=`, then `lowerdir=` joining every parent upperdir with `:`
    ///   (most-recent parent first).
    #[must_use]
    pub fn mounts(&self, snapshot: &Snapshot) -> Vec<Mount> {
        if snapshot.parent_ids.is_empty() {
            return vec![Mount::bind(self.upper_path(&snapshot.id), snapshot.kind)];
        }

        if snapshot.kind == Kind::View && snapshot.parent_ids.len() == 1 {
            // A read-only view with a single parent has no upperdir of its own,
            // so overlay would have nothing to stack — bind the parent directly.
            return vec![Mount::bind(
                self.upper_path(&snapshot.parent_ids[0]),
                Kind::View,
            )];
        }

        let mut options = Vec::new();
        if self.index_off {
            options.push("index=off".to_string());
        }
        if self.userxattr {
            options.push("userxattr".to_string());
        }
        if snapshot.kind == Kind::Active {
            options.push(format!("workdir={}", self.work_path(&snapshot.id)));
            options.push(format!("upperdir={}", self.upper_path(&snapshot.id)));
        }
        let lower = snapshot
            .parent_ids
            .iter()
            .map(|p| self.upper_path(p))
            .collect::<Vec<_>>()
            .join(":");
        options.push(format!("lowerdir={lower}"));

        vec![Mount {
            mount_type: "overlay".to_string(),
            source: "overlay".to_string(),
            options,
        }]
    }
}

/// The native snapshotter's mount-assembly decision.
///
/// Mirrors `snapshots/native/native.go` `mounts()`: every snapshot is a full
/// copy under `<root>/snapshots/<id>`, so a snapshot is always a single bind
/// mount — of its own dir when writable or a base layer, of its immediate
/// parent's dir for a read-only view.
#[derive(Debug, Clone)]
pub struct NativeSnapshotter {
    root: String,
}

impl NativeSnapshotter {
    /// A native snapshotter rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<String>) -> Self {
        Self { root: root.into() }
    }

    /// The directory owning snapshot `id`: `<root>/snapshots/<id>`.
    #[must_use]
    pub fn snapshot_dir(&self, id: &str) -> String {
        snapshot_dir(&self.root, id)
    }

    /// Computes the (single) bind mount to apply for `snapshot`.
    ///
    /// The source is the snapshot's own dir when it is [`Kind::Active`] or has
    /// no parents; otherwise (a read-only view with parents) it is the
    /// immediate parent's dir. The flag is `ro` for a view, else `rw`.
    #[must_use]
    pub fn mounts(&self, snapshot: &Snapshot) -> Vec<Mount> {
        let source = if snapshot.parent_ids.is_empty() || snapshot.kind == Kind::Active {
            self.snapshot_dir(&snapshot.id)
        } else {
            self.snapshot_dir(&snapshot.parent_ids[0])
        };
        vec![Mount::bind(source, snapshot.kind)]
    }
}

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
        assert_eq!(
            mounts[0].options,
            vec!["ro".to_string(), "rbind".to_string()]
        );
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
        // A multi-parent view carries only the lowerdir option, so the whole
        // option vec is exactly the parent join (most-recent parent first).
        let mounts = o.mounts(&snap("3", Kind::View, &["2", "1", "0"]));
        assert_eq!(
            mounts[0].options,
            vec![
                "lowerdir=/var/lib/overlay/snapshots/2/fs:/var/lib/overlay/snapshots/1/fs:/var/lib/overlay/snapshots/0/fs"
                    .to_string(),
            ]
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
        assert_eq!(
            mounts[0].options,
            vec!["ro".to_string(), "rbind".to_string()]
        );
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
