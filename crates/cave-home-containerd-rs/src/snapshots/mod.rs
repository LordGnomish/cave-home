// SPDX-License-Identifier: Apache-2.0
//! Snapshotter — line-by-line port of containerd's
//! `plugins/snapshots/overlay/overlay.go`. Phase 1: in-memory metastore
//! + on-disk overlay layout (`fs/`, `work/`); the actual `mount(2)`
//! syscall is the caller's responsibility (Phase 1b will add a thin
//! mount helper).

pub mod overlay;

pub use overlay::{Info, Kind, Mount, SnapshotError, Snapshotter};
