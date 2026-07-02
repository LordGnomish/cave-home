// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the DaemonSet controller — one pod per node
//! (`pkg/controller/daemonset` contract).

use cave_home_controller_manager_rs::apis::{Cluster, DaemonSet, DaemonSetSpec, Node, PodPhase, PodTemplateSpec};
use cave_home_controller_manager_rs::controllers::daemonset::{DaemonSetController, DS_NODE_LABEL};
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::{Object, ObjectMeta};

fn sel() -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), "agent".to_owned());
    m
}

fn ds() -> DaemonSet {
    DaemonSet::new(
        ObjectMeta::new("agent", "kube-system", ""),
        DaemonSetSpec {
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("app", "agent")]),
            node_selector: std::collections::BTreeMap::new(),
        },
    )
}

/// A node carrying a single label key=value.
fn labeled_node(name: &str, key: &str, value: &str) -> Node {
    let mut n = Node::new(name, "", 0);
    n.meta.labels.insert(key.to_owned(), value.to_owned());
    n
}

fn nodes_covered(c: &Cluster, ds_uid: &str) -> Vec<String> {
    let mut v: Vec<String> = c
        .pods
        .list_owned_by(ds_uid)
        .iter()
        .filter_map(|p| p.meta().labels.get(DS_NODE_LABEL).cloned())
        .collect();
    v.sort();
    v
}

#[test]
fn schedules_one_pod_per_node() {
    let mut c = Cluster::new();
    for n in ["n1", "n2", "n3"] {
        c.nodes.create(Node::new(n, "", 0));
    }
    let d = c.daemonsets.create(ds());
    let mut ctrl = DaemonSetController::new();

    assert_eq!(ctrl.reconcile("kube-system/agent", &mut c, 0), Outcome::Done);
    assert_eq!(nodes_covered(&c, &d.meta().uid), vec!["n1", "n2", "n3"]);
    // Each pod carries the daemonset template label and a controller owner ref.
    for p in c.pods.list_owned_by(&d.meta().uid) {
        assert_eq!(p.meta().labels.get("app").map(String::as_str), Some("agent"));
    }
}

#[test]
fn a_new_node_gets_a_pod() {
    let mut c = Cluster::new();
    c.nodes.create(Node::new("n1", "", 0));
    let d = c.daemonsets.create(ds());
    let mut ctrl = DaemonSetController::new();
    ctrl.reconcile("kube-system/agent", &mut c, 0);
    assert_eq!(nodes_covered(&c, &d.meta().uid), vec!["n1"]);

    c.nodes.create(Node::new("n2", "", 0));
    ctrl.reconcile("kube-system/agent", &mut c, 1);
    assert_eq!(nodes_covered(&c, &d.meta().uid), vec!["n1", "n2"], "new node covered");
}

#[test]
fn a_removed_node_has_its_pod_deleted() {
    let mut c = Cluster::new();
    c.nodes.create(Node::new("n1", "", 0));
    c.nodes.create(Node::new("n2", "", 0));
    let d = c.daemonsets.create(ds());
    let mut ctrl = DaemonSetController::new();
    ctrl.reconcile("kube-system/agent", &mut c, 0);
    assert_eq!(nodes_covered(&c, &d.meta().uid).len(), 2);

    c.nodes.delete("n2");
    ctrl.reconcile("kube-system/agent", &mut c, 1);
    assert_eq!(nodes_covered(&c, &d.meta().uid), vec!["n1"], "orphaned pod for n2 deleted");
}

#[test]
fn steady_state_is_a_noop() {
    let mut c = Cluster::new();
    c.nodes.create(Node::new("n1", "", 0));
    let d = c.daemonsets.create(ds());
    let mut ctrl = DaemonSetController::new();
    ctrl.reconcile("kube-system/agent", &mut c, 0);
    let before: Vec<_> = c.pods.list_owned_by(&d.meta().uid).iter().map(|p| p.meta().uid.clone()).collect();
    ctrl.reconcile("kube-system/agent", &mut c, 1);
    let after: Vec<_> = c.pods.list_owned_by(&d.meta().uid).iter().map(|p| p.meta().uid.clone()).collect();
    assert_eq!(before, after, "no pod churn at steady state");
}

#[test]
fn missing_daemonset_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = DaemonSetController::new();
    assert_eq!(ctrl.reconcile("kube-system/ghost", &mut c, 0), Outcome::Done);
}

// --- nodeSelector eligibility -------------------------------------------

fn ds_with_node_selector(key: &str, value: &str) -> DaemonSet {
    let mut node_selector = std::collections::BTreeMap::new();
    node_selector.insert(key.to_owned(), value.to_owned());
    DaemonSet::new(
        ObjectMeta::new("agent", "kube-system", ""),
        DaemonSetSpec {
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("app", "agent")]),
            node_selector,
        },
    )
}

#[test]
fn only_nodes_matching_the_node_selector_get_a_pod() {
    let mut c = Cluster::new();
    c.nodes.create(labeled_node("gpu1", "gpu", "true"));
    c.nodes.create(labeled_node("gpu2", "gpu", "true"));
    c.nodes.create(Node::new("cpu1", "", 0)); // no gpu label
    let d = c.daemonsets.create(ds_with_node_selector("gpu", "true"));
    let mut ctrl = DaemonSetController::new();

    ctrl.reconcile("kube-system/agent", &mut c, 0);
    assert_eq!(nodes_covered(&c, &d.meta().uid), vec!["gpu1", "gpu2"], "only gpu nodes scheduled");
}

#[test]
fn a_pod_is_removed_when_its_node_stops_matching_the_selector() {
    let mut c = Cluster::new();
    c.nodes.create(labeled_node("n1", "gpu", "true"));
    let d = c.daemonsets.create(ds_with_node_selector("gpu", "true"));
    let mut ctrl = DaemonSetController::new();
    ctrl.reconcile("kube-system/agent", &mut c, 0);
    assert_eq!(nodes_covered(&c, &d.meta().uid), vec!["n1"]);

    // The node loses its gpu label: it is no longer eligible.
    let mut n1 = c.nodes.get("n1").unwrap();
    n1.meta.labels.clear();
    c.nodes.update(n1);
    ctrl.reconcile("kube-system/agent", &mut c, 1);
    assert!(nodes_covered(&c, &d.meta().uid).is_empty(), "pod removed from the now-ineligible node");
}

// --- status -------------------------------------------------------------

#[test]
fn status_counts_desired_current_ready_and_misscheduled() {
    let mut c = Cluster::new();
    for n in ["n1", "n2"] {
        c.nodes.create(Node::new(n, "", 0));
    }
    let d = c.daemonsets.create(ds());
    let mut ctrl = DaemonSetController::new();
    ctrl.reconcile("kube-system/agent", &mut c, 0);

    // Mark the pod on n1 ready.
    let mut pods = c.pods.list_owned_by(&d.meta().uid);
    pods.sort_by_key(|p| p.meta().labels.get(DS_NODE_LABEL).cloned().unwrap_or_default());
    let mut p = pods[0].clone();
    p.status.phase = PodPhase::Running;
    p.status.ready = true;
    c.pods.update(p);

    ctrl.reconcile("kube-system/agent", &mut c, 1);
    let st = c.daemonsets.get("kube-system/agent").unwrap().status;
    assert_eq!(st.desired_number_scheduled, 2, "two eligible nodes");
    assert_eq!(st.current_number_scheduled, 2, "both have a pod");
    assert_eq!(st.number_ready, 1, "one pod ready");
    assert_eq!(st.number_misscheduled, 0);
}
