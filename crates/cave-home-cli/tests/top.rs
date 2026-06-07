// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`cavehomectl top`** surface — the `kubectl top`
//! equivalent backed by the in-process metrics_server pipeline.
//!
//! `cavehomectl top nodes` and `cavehomectl top pods` render the node / pod
//! CPU + memory tables. This drives the pure table-rendering half (column
//! layout, units, the empty-set message); the binary wires the live
//! `metrics_server` `NodeMetrics` / `PodMetrics` into these rows.

use cave_home_cli::top::{render_top_nodes, render_top_pods, top_subcommands, NodeTopRow, PodTopRow};

fn cells(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

#[test]
fn top_has_nodes_and_pods_subcommands() {
    let sc = top_subcommands();
    assert!(sc.contains(&"nodes"));
    assert!(sc.contains(&"pods"));
}

#[test]
fn top_nodes_renders_header_and_rows() {
    let rows = vec![
        NodeTopRow::new("hub-1", 250, Some(12), 128, Some(25)),
        NodeTopRow::new("worker-2", 1000, Some(50), 512, Some(40)),
    ];
    let out = render_top_nodes(&rows);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(cells(lines[0]), vec!["NAME", "CPU(cores)", "CPU%", "MEMORY(bytes)", "MEMORY%"]);
    assert_eq!(cells(lines[1]), vec!["hub-1", "250m", "12%", "128Mi", "25%"]);
    assert_eq!(cells(lines[2]), vec!["worker-2", "1000m", "50%", "512Mi", "40%"]);
}

#[test]
fn top_nodes_unknown_percent_renders_placeholder() {
    let rows = vec![NodeTopRow::new("hub-1", 250, None, 128, None)];
    let out = render_top_nodes(&rows);
    let data = cells(out.lines().nth(1).expect("a data row"));
    assert_eq!(data, vec!["hub-1", "250m", "<unknown>", "128Mi", "<unknown>"]);
}

#[test]
fn top_nodes_empty_says_no_resources() {
    assert_eq!(render_top_nodes(&[]), "No resources found");
}

#[test]
fn top_pods_without_namespace_column() {
    let rows = vec![PodTopRow::new("apps", "web", 250, 128)];
    let out = render_top_pods(&rows, false);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(cells(lines[0]), vec!["NAME", "CPU(cores)", "MEMORY(bytes)"]);
    assert_eq!(cells(lines[1]), vec!["web", "250m", "128Mi"]);
}

#[test]
fn top_pods_with_namespace_column() {
    let rows = vec![PodTopRow::new("apps", "web", 250, 128)];
    let out = render_top_pods(&rows, true);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(cells(lines[0]), vec!["NAMESPACE", "NAME", "CPU(cores)", "MEMORY(bytes)"]);
    assert_eq!(cells(lines[1]), vec!["apps", "web", "250m", "128Mi"]);
}

#[test]
fn top_pods_empty_says_no_resources() {
    assert_eq!(render_top_pods(&[], true), "No resources found");
}

#[test]
fn columns_are_left_aligned_and_padded() {
    // The long name pushes the CPU column right; both data rows must start their
    // CPU cell at the same offset (left-aligned, padded to the widest NAME).
    let rows = vec![
        NodeTopRow::new("a", 1, Some(1), 1, Some(1)),
        NodeTopRow::new("a-very-long-node-name", 2, Some(2), 2, Some(2)),
    ];
    let out = render_top_nodes(&rows);
    let lines: Vec<&str> = out.lines().collect();
    let cpu_col = lines[0].find("CPU(cores)").expect("header has CPU col");
    // Each data row has "250m"-style value beginning exactly at the header offset.
    assert_eq!(&lines[1][cpu_col..cpu_col + 2], "1m");
    assert_eq!(&lines[2][cpu_col..cpu_col + 2], "2m");
}
