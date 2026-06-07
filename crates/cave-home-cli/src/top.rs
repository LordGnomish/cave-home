// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl top nodes` / `cavehomectl top pods` — the `kubectl top`
//! equivalent backed by the in-process `metrics_server` pipeline.
//!
//! This module is the pure table renderer: it lays out the node / pod CPU +
//! memory tables exactly as `kubectl top` does — `CPU(cores)` in millicores,
//! `MEMORY(bytes)` in MiB, left-aligned columns padded to the widest cell, and
//! `No resources found` for an empty set. The live `metrics_server`
//! `NodeMetrics` / `PodMetrics` are mapped into [`NodeTopRow`] / [`PodTopRow`]
//! by the binary wiring; keeping the renderer row-based keeps this crate free of
//! a dependency on the orchestration crate (strict crate isolation).

/// The empty-set line both tables print, matching `kubectl`.
const EMPTY: &str = "No resources found";

/// The placeholder a utilisation percentage shows when node capacity is unknown.
const UNKNOWN: &str = "<unknown>";

/// `cavehomectl top` sub-commands.
#[must_use]
pub fn top_subcommands() -> Vec<&'static str> {
    vec!["nodes", "pods"]
}

/// One row of `top nodes`: a node and its CPU / memory usage (+ utilisation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeTopRow {
    /// Node name.
    pub name: String,
    /// CPU usage in millicores.
    pub cpu_millicores: u64,
    /// CPU utilisation percent of allocatable, if capacity is known.
    pub cpu_percent: Option<u8>,
    /// Memory usage in MiB.
    pub memory_mib: u64,
    /// Memory utilisation percent of allocatable, if capacity is known.
    pub memory_percent: Option<u8>,
}

impl NodeTopRow {
    /// Construct a node row.
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
}

/// One row of `top pods`: a pod and its summed CPU / memory usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodTopRow {
    /// Pod namespace.
    pub namespace: String,
    /// Pod name.
    pub name: String,
    /// CPU usage in millicores (sum of containers).
    pub cpu_millicores: u64,
    /// Memory usage in MiB (sum of containers).
    pub memory_mib: u64,
}

impl PodTopRow {
    /// Construct a pod row.
    #[must_use]
    pub fn new(namespace: &str, name: &str, cpu_millicores: u64, memory_mib: u64) -> Self {
        Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            cpu_millicores,
            memory_mib,
        }
    }
}

/// Format millicores as the `CPU(cores)` cell (e.g. `250m`).
fn cpu_cell(millicores: u64) -> String {
    format!("{millicores}m")
}

/// Format MiB as the `MEMORY(bytes)` cell (e.g. `128Mi`).
fn mem_cell(mib: u64) -> String {
    format!("{mib}Mi")
}

/// Format an optional utilisation percent (`25%` or `<unknown>`).
fn pct_cell(pct: Option<u8>) -> String {
    pct.map_or_else(|| UNKNOWN.to_string(), |p| format!("{p}%"))
}

/// Render `rows` of `cells` (the first row is the header) as a left-aligned,
/// space-padded table. Columns are padded to the widest cell; the last column
/// is not padded. Three spaces separate columns.
fn render_table(rows: &[Vec<String>]) -> String {
    let cols = rows.first().map_or(0, Vec::len);
    let mut widths = vec![0usize; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let mut out = String::new();
    for (r, row) in rows.iter().enumerate() {
        if r > 0 {
            out.push('\n');
        }
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("   ");
            }
            out.push_str(cell);
            if i + 1 != row.len() {
                // Pad non-final columns to the column width (left-aligned).
                out.push_str(&" ".repeat(widths[i] - cell.len()));
            }
        }
    }
    out
}

/// Render the `top nodes` table, or `No resources found` when empty.
#[must_use]
pub fn render_top_nodes(rows: &[NodeTopRow]) -> String {
    if rows.is_empty() {
        return EMPTY.to_string();
    }
    let mut table = vec![vec![
        "NAME".to_string(),
        "CPU(cores)".to_string(),
        "CPU%".to_string(),
        "MEMORY(bytes)".to_string(),
        "MEMORY%".to_string(),
    ]];
    for r in rows {
        table.push(vec![
            r.name.clone(),
            cpu_cell(r.cpu_millicores),
            pct_cell(r.cpu_percent),
            mem_cell(r.memory_mib),
            pct_cell(r.memory_percent),
        ]);
    }
    render_table(&table)
}

/// Render the `top pods` table, optionally with a leading `NAMESPACE` column,
/// or `No resources found` when empty.
#[must_use]
pub fn render_top_pods(rows: &[PodTopRow], with_namespace: bool) -> String {
    if rows.is_empty() {
        return EMPTY.to_string();
    }
    let mut header = Vec::new();
    if with_namespace {
        header.push("NAMESPACE".to_string());
    }
    header.extend([
        "NAME".to_string(),
        "CPU(cores)".to_string(),
        "MEMORY(bytes)".to_string(),
    ]);
    let mut table = vec![header];
    for r in rows {
        let mut row = Vec::new();
        if with_namespace {
            row.push(r.namespace.clone());
        }
        row.extend([
            r.name.clone(),
            cpu_cell(r.cpu_millicores),
            mem_cell(r.memory_mib),
        ]);
        table.push(row);
    }
    render_table(&table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_format_with_units() {
        assert_eq!(cpu_cell(250), "250m");
        assert_eq!(mem_cell(128), "128Mi");
        assert_eq!(pct_cell(Some(25)), "25%");
        assert_eq!(pct_cell(None), "<unknown>");
    }

    #[test]
    fn render_table_pads_non_final_columns() {
        let rows = vec![
            vec!["A".to_string(), "B".to_string()],
            vec!["longer".to_string(), "x".to_string()],
        ];
        let out = render_table(&rows);
        // "A" is padded to width 6 ("longer"), then 3 spaces, then "B".
        assert_eq!(out.lines().next().expect("header"), "A        B");
    }
}
