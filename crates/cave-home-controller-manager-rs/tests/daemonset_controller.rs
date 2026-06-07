// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the DaemonSet controller — one pod per node
//! (`pkg/controller/daemonset` contract).

use cave_home_controller_manager_rs::apis::{Cluster, DaemonSet, DaemonSetSpec, Node, PodTemplateSpec};
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
        DaemonSetSpec { selector: sel(), template: PodTemplateSpec::with_labels(&[("app", "agent")]) },
    )
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
