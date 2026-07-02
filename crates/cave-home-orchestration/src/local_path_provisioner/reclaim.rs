//! The `Delete` / reclaim decision (port of the upstream `deleteFor` +
//! `getPathAndNodeForPV`).
//!
//! When a `PersistentVolume` is released, upstream decides whether to reclaim it:
//! a `Retain` policy keeps the directory; any other policy recovers the on-node
//! path (from the PV's `hostPath` / `local` source) and the node (from the PV's
//! node-affinity term) and launches a teardown helper pod. cave-home ports the
//! *decision* — retain-vs-teardown plus the path/node recovery and its validity
//! checks — here; the upstream "does the node still exist?" API probe and the
//! actual teardown-pod creation are ADR-004 phase-1b.

use super::provision::{PvSpec, ReclaimPolicy, VolumeSource};

/// What to do when a PV is released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteDecision {
    /// Keep the backing directory (the `Retain` policy).
    Retain,
    /// Remove the backing directory via a teardown helper pod.
    Teardown {
        /// The on-node directory to remove.
        path: String,
        /// The node it lives on (empty on a shared filesystem).
        node: String,
    },
}

/// Recover the `(path, node)` for a released PV (upstream `getPathAndNodeForPV`).
///
/// The path comes from the PV's `hostPath` / `local` source. On a shared
/// filesystem the node is empty (no affinity); otherwise it is read from the
/// PV's node-affinity term, which must carry exactly one non-empty value.
///
/// # Errors
/// [`ReclaimError::NoNodeAffinity`] when a local-FS PV has no affinity term,
/// [`ReclaimError::MultipleAffinityValues`] when the term has more than one
/// value, or [`ReclaimError::CannotFindAffinitedNode`] when the value is empty.
pub fn path_and_node_for_pv(pv: &PvSpec, shared_fs: bool) -> Result<(String, String), ReclaimError> {
    let path = match &pv.source {
        VolumeSource::HostPath { path } | VolumeSource::Local { path } => path.clone(),
    };

    if shared_fs {
        // Shared FS: no affinity, any node can reach the path.
        return Ok((path, String::new()));
    }

    let affinity = pv
        .node_affinity
        .as_ref()
        .ok_or(ReclaimError::NoNodeAffinity)?;
    if affinity.values.len() != 1 {
        return Err(ReclaimError::MultipleAffinityValues);
    }
    let node = affinity.values[0].clone();
    if node.is_empty() {
        return Err(ReclaimError::CannotFindAffinitedNode);
    }
    Ok((path, node))
}

/// Decide how to reclaim a released PV (upstream `deleteFor`).
///
/// A [`ReclaimPolicy::Retain`] PV is kept; any other policy tears down, using the
/// path/node recovered by [`path_and_node_for_pv`].
///
/// # Errors
/// Propagates [`ReclaimError`] from path/node recovery for a torn-down PV.
pub fn delete_decision(pv: &PvSpec, shared_fs: bool) -> Result<DeleteDecision, ReclaimError> {
    if pv.reclaim_policy == ReclaimPolicy::Retain {
        return Ok(DeleteDecision::Retain);
    }
    let (path, node) = path_and_node_for_pv(pv, shared_fs)?;
    Ok(DeleteDecision::Teardown { path, node })
}

/// A reclaim-decision failure (upstream returns Go `error` strings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimError {
    /// A local-FS PV carried no node-affinity term (upstream "no `NodeAffinity`
    /// set" / "no `NodeAffinity.Required` set").
    NoNodeAffinity,
    /// The node-affinity term had more than one value (upstream "multiple values
    /// for the node affinity").
    MultipleAffinityValues,
    /// No affinited node could be determined (upstream "cannot find affinited
    /// node").
    CannotFindAffinitedNode,
}

impl core::fmt::Display for ReclaimError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoNodeAffinity => f.write_str("no NodeAffinity set"),
            Self::MultipleAffinityValues => f.write_str("multiple values for the node affinity"),
            Self::CannotFindAffinitedNode => f.write_str("cannot find affinited node"),
        }
    }
}

impl std::error::Error for ReclaimError {}
