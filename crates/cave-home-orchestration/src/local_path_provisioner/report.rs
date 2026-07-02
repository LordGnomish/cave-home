//! The storage **report view-model** — the PV/PVC rows that the operator
//! surfaces render (Tracks 2 & 3).
//!
//! `cavectl orchestration storage list-pvs` lists every provisioned volume, and
//! `cavectl orchestration storage describe <pvc>` (and the Portal Storage page)
//! shows one volume's detail including its on-node `hostPath`. This module turns
//! the decision-core's [`PvSpec`] into flat, render-ready rows; it is pure data,
//! carrying no formatting and no i18n (this is developer-facing infrastructure,
//! Charter §6.3).

use super::metrics::PvPhase;
use super::provision::{PvSpec, ReclaimPolicy, VolumeSource};

/// One render-ready row describing a provisioned volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PvRow {
    /// The PV name.
    pub name: String,
    /// The bound claim's namespace.
    pub pvc_namespace: String,
    /// The bound claim's name.
    pub pvc_name: String,
    /// The node the volume lives on (empty on a shared filesystem).
    pub node: String,
    /// The on-node directory path (the `hostPath` / `local` source).
    pub host_path: String,
    /// The PV lifecycle phase.
    pub phase: PvPhase,
    /// The capacity in bytes.
    pub capacity_bytes: u64,
    /// The reclaim policy.
    pub reclaim_policy: ReclaimPolicy,
}

impl PvRow {
    /// Build a row from a [`PvSpec`] and the claim coordinates / observed phase.
    ///
    /// The host path comes from the volume source; the node from the PV's
    /// node-affinity term (the first value), or empty when there is none (shared
    /// filesystem).
    #[must_use]
    pub fn from_pv_spec(pv: &PvSpec, pvc_namespace: &str, pvc_name: &str, phase: PvPhase) -> Self {
        let host_path = match &pv.source {
            VolumeSource::HostPath { path } | VolumeSource::Local { path } => path.clone(),
        };
        let node = pv
            .node_affinity
            .as_ref()
            .and_then(|a| a.values.first().cloned())
            .unwrap_or_default();
        Self {
            name: pv.name.clone(),
            pvc_namespace: pvc_namespace.to_string(),
            pvc_name: pvc_name.to_string(),
            node,
            host_path,
            phase,
            capacity_bytes: pv.capacity_bytes,
            reclaim_policy: pv.reclaim_policy,
        }
    }
}

/// The whole storage report: the set of provisioned-volume rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageReport {
    rows: Vec<PvRow>,
}

impl StorageReport {
    /// Build a report from a set of rows.
    #[must_use]
    pub const fn new(rows: Vec<PvRow>) -> Self {
        Self { rows }
    }

    /// All rows (for `list-pvs` / the Portal table).
    #[must_use]
    pub fn rows(&self) -> &[PvRow] {
        &self.rows
    }

    /// The number of provisioned volumes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there are no provisioned volumes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Find the row for a bound claim by `(namespace, name)` (for `describe`).
    #[must_use]
    pub fn find_by_pvc(&self, namespace: &str, pvc_name: &str) -> Option<&PvRow> {
        self.rows
            .iter()
            .find(|r| r.pvc_namespace == namespace && r.pvc_name == pvc_name)
    }
}
