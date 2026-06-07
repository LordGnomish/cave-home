// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the ServiceAccount controller — ensures every
//! namespace has a `default` ServiceAccount (`pkg/controller/serviceaccount`).

use cave_home_controller_manager_rs::apis::{Cluster, Namespace};
use cave_home_controller_manager_rs::controllers::serviceaccount::ServiceAccountController;
use cave_home_controller_manager_rs::reconcile::Outcome;

#[test]
fn creates_a_default_service_account_for_a_namespace() {
    let mut c = Cluster::new();
    c.namespaces.create(Namespace::new("team-a", ""));
    let mut ctrl = ServiceAccountController::new();

    assert_eq!(ctrl.reconcile("team-a", &mut c, 0), Outcome::Done);
    let sa = c.service_accounts.get("team-a/default").expect("default SA created");
    assert_eq!(sa.meta.namespace, "team-a");
}

#[test]
fn is_idempotent_when_default_already_exists() {
    let mut c = Cluster::new();
    c.namespaces.create(Namespace::new("team-a", ""));
    let mut ctrl = ServiceAccountController::new();
    ctrl.reconcile("team-a", &mut c, 0);
    let uid = c.service_accounts.get("team-a/default").unwrap().meta.uid;
    ctrl.reconcile("team-a", &mut c, 1);
    assert_eq!(c.service_accounts.list().len(), 1, "no duplicate default SA");
    assert_eq!(c.service_accounts.get("team-a/default").unwrap().meta.uid, uid, "same object");
}

#[test]
fn terminating_namespace_gets_no_service_account() {
    let mut c = Cluster::new();
    let mut ns = Namespace::new("dying", "");
    ns.meta.deletion_timestamp = Some(5);
    c.namespaces.create(ns);
    let mut ctrl = ServiceAccountController::new();
    ctrl.reconcile("dying", &mut c, 10);
    assert!(c.service_accounts.get("dying/default").is_none(), "no SA for a terminating namespace");
}

#[test]
fn missing_namespace_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = ServiceAccountController::new();
    assert_eq!(ctrl.reconcile("ghost", &mut c, 0), Outcome::Done);
}
