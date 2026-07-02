// SPDX-License-Identifier: Apache-2.0
//! The Portal **Metrics page** view-model — the developer-only live node /
//! workload CPU + memory dashboard.
//!
//! The cluster resource metrics (the `kubectl top`-class data the in-process
//! `metrics_server` produces) are power-user content, so this page is
//! **developer-only**: [`MetricsPage::into_view`] yields a developer-only
//! [`View`] carrying a [`Card::ClusterMetrics`], which the [`crate::dashboard`]
//! layout engine drops entirely from resident / mobile output (Charter §6.3).
//!
//! This crate is a pure UI model with no network dependency: the live usage
//! ([`NodeUsageTile`] / [`WorkloadUsageTile`]) is fed in by the Portal backend,
//! which maps the `metrics_server` `NodeMetrics` / `PodMetrics` into these tiles.

use crate::card::Card;
use crate::dashboard::View;

/// The placeholder a utilisation percentage shows when capacity is unknown.
const UNKNOWN: &str = "—";

/// One node's usage tile on the Metrics page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeUsageTile {
    /// Friendly node label ("Hub", "Backup hub").
    pub name: String,
    /// CPU usage in millicores.
    pub cpu_millicores: u64,
    /// CPU utilisation percent, if capacity is known.
    pub cpu_percent: Option<u8>,
    /// Memory usage in MiB.
    pub memory_mib: u64,
    /// Memory utilisation percent, if capacity is known.
    pub memory_percent: Option<u8>,
}

impl NodeUsageTile {
    /// Construct a node usage tile.
    #[must_use]
    pub fn new(
        name: &str,
        cpu_millicores: u64,
        cpu_percent: Option<u8>,
        memory_mib: u64,
        memory_percent: Option<u8>,
    ) -> Self {
        Self {
            name: name.to_string(),
            cpu_millicores,
            cpu_percent,
            memory_mib,
            memory_percent,
        }
    }

    /// The CPU usage label (`250m`).
    #[must_use]
    pub fn cpu_label(&self) -> String {
        format!("{}m", self.cpu_millicores)
    }

    /// The memory usage label (`128Mi`).
    #[must_use]
    pub fn memory_label(&self) -> String {
        format!("{}Mi", self.memory_mib)
    }

    /// The CPU utilisation label (`12%` or `—`).
    #[must_use]
    pub fn cpu_percent_label(&self) -> String {
        percent_label(self.cpu_percent)
    }

    /// The memory utilisation label (`25%` or `—`).
    #[must_use]
    pub fn memory_percent_label(&self) -> String {
        percent_label(self.memory_percent)
    }
}

/// One workload's usage tile on the Metrics page (a device group / service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadUsageTile {
    /// Friendly workload label ("Cameras", "Voice").
    pub name: String,
    /// CPU usage in millicores.
    pub cpu_millicores: u64,
    /// Memory usage in MiB.
    pub memory_mib: u64,
}

impl WorkloadUsageTile {
    /// Construct a workload usage tile.
    #[must_use]
    pub fn new(name: &str, cpu_millicores: u64, memory_mib: u64) -> Self {
        Self {
            name: name.to_string(),
            cpu_millicores,
            memory_mib,
        }
    }

    /// The CPU usage label (`500m`).
    #[must_use]
    pub fn cpu_label(&self) -> String {
        format!("{}m", self.cpu_millicores)
    }

    /// The memory usage label (`256Mi`).
    #[must_use]
    pub fn memory_label(&self) -> String {
        format!("{}Mi", self.memory_mib)
    }
}

/// Format an optional utilisation percent (`25%` or the unknown placeholder).
fn percent_label(pct: Option<u8>) -> String {
    pct.map_or_else(|| UNKNOWN.to_string(), |p| format!("{p}%"))
}

/// The Metrics page model: node tiles + workload tiles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricsPage {
    /// Per-node usage tiles.
    pub nodes: Vec<NodeUsageTile>,
    /// Per-workload usage tiles.
    pub workloads: Vec<WorkloadUsageTile>,
}

impl MetricsPage {
    /// Build a Metrics page from node and workload tiles.
    #[must_use]
    pub const fn new(nodes: Vec<NodeUsageTile>, workloads: Vec<WorkloadUsageTile>) -> Self {
        Self { nodes, workloads }
    }

    /// How many nodes the page shows.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// How many workloads the page shows.
    #[must_use]
    pub fn workload_count(&self) -> usize {
        self.workloads.len()
    }

    /// Render the page as a **developer-only** dashboard [`View`] carrying the
    /// [`Card::ClusterMetrics`] slot. The layout engine drops it for residents
    /// and on mobile (Charter §6.3).
    #[must_use]
    pub fn into_view(self) -> View {
        View::developer("Metrics", "activity", vec![Card::ClusterMetrics])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_label_handles_known_and_unknown() {
        assert_eq!(percent_label(Some(40)), "40%");
        assert_eq!(percent_label(None), "—");
    }

    #[test]
    fn empty_page_has_zero_counts() {
        let page = MetricsPage::default();
        assert_eq!(page.node_count(), 0);
        assert_eq!(page.workload_count(), 0);
    }

    #[test]
    fn view_is_developer_only_and_titled() {
        let view = MetricsPage::default().into_view();
        assert!(view.developer_only);
        assert_eq!(view.title, "Metrics");
    }
}
