// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the Namespace controller — drains a terminating
//! namespace's content then drops the finalizer (`pkg/controller/namespace`).

use cave_home_controller_manager_rs::apis::{Cluster, Namespace, Pod, ReplicaSet, ReplicaSetSpec};
use cave_home_controller_manager_rs::controllers::cleanup::NAMESPACE_FINALIZER;
use cave_home_controller_manager_rs::controllers::namespace::NamespaceController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::ObjectMeta;

fn terminating_ns(name: &str) -> Namespace {
    let mut ns = Namespace::new(name, "");
    ns.meta.finalizers.push(NAMESPACE_FINALIZER.to_owned());
    ns.meta.deletion_timestamp = Some(100);
    ns
}

#[test]
fn active_namespace_is_left_alone() {
    let mut c = Cluster::new();
    let mut ns = Namespace::new("live", "");
    ns.meta.finalizers.push(NAMESPACE_FINALIZER.to_owned());
    c.namespaces.create(ns);
    c.pods.create(Pod::new(ObjectMeta::new("p", "live", "")));
    let mut ctrl = NamespaceController::new();

    assert_eq!(ctrl.reconcile("live", &mut c, 0), Outcome::Done);
    assert!(c.pods.get("live/p").is_some(), "content untouched");
    assert!(c.namespaces.get("live").unwrap().meta.finalizers.contains(&NAMESPACE_FINALIZER.to_owned()));
}

#[test]
fn terminating_namespace_deletes_its_content_first() {
    let mut c = Cluster::new();
    c.namespaces.create(terminating_ns("doomed"));
    c.pods.create(Pod::new(ObjectMeta::new("p1", "doomed", "")));
    c.pods.create(Pod::new(ObjectMeta::new("p2", "doomed", "")));
    c.replicasets.create(ReplicaSet::new(ObjectMeta::new("rs", "doomed", ""), ReplicaSetSpec::default()));
    // A pod in another namespace must survive.
    c.pods.create(Pod::new(ObjectMeta::new("safe", "other", "")));
    let mut ctrl = NamespaceController::new();

    ctrl.reconcile("doomed", &mut c, 200);
    assert!(c.pods.get("doomed/p1").is_none() && c.pods.get("doomed/p2").is_none(), "pods purged");
    assert!(c.replicasets.get("doomed/rs").is_none(), "RS purged");
    assert!(c.pods.get("other/safe").is_some(), "other namespace untouched");
    // Finalizer not yet removed in the same pass that still saw content.
    let ns = c.namespaces.get("doomed").unwrap();
    assert!(ns.meta.finalizers.contains(&NAMESPACE_FINALIZER.to_owned()), "finalizer waits for empty");
}

#[test]
fn empty_terminating_namespace_drops_the_finalizer() {
    let mut c = Cluster::new();
    c.namespaces.create(terminating_ns("doomed"));
    let mut ctrl = NamespaceController::new();
    ctrl.reconcile("doomed", &mut c, 200);
    let ns = c.namespaces.get("doomed").unwrap();
    assert!(!ns.meta.finalizers.contains(&NAMESPACE_FINALIZER.to_owned()), "finalizer removed when empty");
}

#[test]
fn drains_then_finalizes_over_two_reconciles() {
    let mut c = Cluster::new();
    c.namespaces.create(terminating_ns("doomed"));
    c.pods.create(Pod::new(ObjectMeta::new("p", "doomed", "")));
    let mut ctrl = NamespaceController::new();
    ctrl.reconcile("doomed", &mut c, 200); // deletes content
    assert!(c.namespaces.get("doomed").unwrap().meta.finalizers.contains(&NAMESPACE_FINALIZER.to_owned()));
    ctrl.reconcile("doomed", &mut c, 201); // now empty -> finalize
    assert!(c.namespaces.get("doomed").unwrap().meta.finalizers.is_empty());
}

#[test]
fn missing_namespace_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = NamespaceController::new();
    assert_eq!(ctrl.reconcile("ghost", &mut c, 0), Outcome::Done);
}
