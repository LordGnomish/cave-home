// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the ReplicaSet controller reconciling against the
//! in-memory apiserver (`pkg/controller/replicaset` behavioural contract).

use cave_home_controller_manager_rs::apis::{
    Cluster, Pod, PodPhase, PodTemplateSpec, ReplicaSet, ReplicaSetSpec,
};
use cave_home_controller_manager_rs::controllers::replicaset::ReplicaSetController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::{Object, ObjectMeta, OwnerReference};

fn sel(app: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), app.to_owned());
    m
}

fn rs(name: &str, replicas: i32) -> ReplicaSet {
    ReplicaSet::new(
        ObjectMeta::new(name, "prod", ""),
        ReplicaSetSpec {
            replicas,
            selector: sel("web"),
            template: PodTemplateSpec::with_labels(&[("app", "web")]),
        },
    )
}

#[test]
fn scale_up_from_zero_creates_pods_owned_by_the_rs() {
    let mut c = Cluster::new();
    let rs = c.replicasets.create(rs("web", 3));
    let mut ctrl = ReplicaSetController::new();

    let out = ctrl.reconcile("prod/web", &mut c, 0);
    assert_eq!(out, Outcome::Done);

    let pods = c.pods.list_owned_by(&rs.meta().uid);
    assert_eq!(pods.len(), 3, "three pods created");
    for p in &pods {
        assert_eq!(p.meta().labels.get("app").map(String::as_str), Some("web"));
        let owner = &p.meta().owner_references[0];
        assert!(owner.controller && owner.block_owner_deletion, "controller+blocking owner ref");
        assert_eq!(owner.uid, rs.meta().uid);
    }
    // Status is written back.
    assert_eq!(c.replicasets.get("prod/web").map(|r| r.status.replicas), Some(3));
}

#[test]
fn steady_state_is_a_noop() {
    let mut c = Cluster::new();
    let rs = c.replicasets.create(rs("web", 2));
    let mut ctrl = ReplicaSetController::new();
    ctrl.reconcile("prod/web", &mut c, 0); // converge
    let before: Vec<_> = c.pods.list().iter().map(|p| p.meta().uid.clone()).collect();
    let out = ctrl.reconcile("prod/web", &mut c, 1); // again
    assert_eq!(out, Outcome::Done);
    let after: Vec<_> = c.pods.list().iter().map(|p| p.meta().uid.clone()).collect();
    assert_eq!(before, after, "no pods created or deleted at steady state");
    let _ = rs;
}

#[test]
fn scale_down_deletes_least_ready_pods_first() {
    let mut c = Cluster::new();
    let rs = c.replicasets.create(rs("web", 1));
    let mut ctrl = ReplicaSetController::new();

    let owner = OwnerReference::to("ReplicaSet", "web", &rs.meta().uid)
        .controller()
        .blocking();
    // A ready running pod (should survive) and two weaker pods (should die).
    let mut running = Pod::new(
        ObjectMeta::new("running", "prod", "p-run").with_label("app", "web").with_owner(owner.clone()),
    );
    running.status.phase = PodPhase::Running;
    running.status.ready = true;
    let pending = Pod::new(
        ObjectMeta::new("pending", "prod", "p-pend").with_label("app", "web").with_owner(owner.clone()),
    );
    let mut not_ready = Pod::new(
        ObjectMeta::new("notready", "prod", "p-nr").with_label("app", "web").with_owner(owner),
    );
    not_ready.status.phase = PodPhase::Running;
    not_ready.status.ready = false;
    c.pods.update(running);
    c.pods.update(pending);
    c.pods.update(not_ready);

    let out = ctrl.reconcile("prod/web", &mut c, 0);
    assert_eq!(out, Outcome::Done);

    let survivors: Vec<_> = c.pods.list().iter().map(|p| p.meta().name.clone()).collect();
    assert_eq!(survivors, vec!["running"], "the ready running pod is kept; weaker pods deleted first");
}

#[test]
fn adopts_a_matching_orphan_pod() {
    let mut c = Cluster::new();
    let rs = c.replicasets.create(rs("web", 1));
    let mut ctrl = ReplicaSetController::new();
    // An orphan pod that matches the selector but has no controller.
    c.pods.create(Pod::new(ObjectMeta::new("legacy", "prod", "").with_label("app", "web")));

    ctrl.reconcile("prod/web", &mut c, 0);

    let adopted = c.pods.get("prod/legacy").expect("orphan still present");
    let owner = adopted
        .meta()
        .owner_references
        .iter()
        .find(|r| r.controller)
        .expect("adopted: now has a controller owner");
    assert_eq!(owner.uid, rs.meta().uid);
    // It satisfied desired replicas, so no extra pod was created.
    assert_eq!(c.pods.list().len(), 1, "adoption avoids creating a redundant pod");
}

#[test]
fn releases_an_owned_pod_that_no_longer_matches() {
    let mut c = Cluster::new();
    let rs = c.replicasets.create(rs("web", 0));
    let mut ctrl = ReplicaSetController::new();
    let owner = OwnerReference::to("ReplicaSet", "web", &rs.meta().uid)
        .controller()
        .blocking();
    // Owned but label no longer matches the selector (app=db).
    c.pods.create(
        Pod::new(ObjectMeta::new("drifted", "prod", "").with_label("app", "db").with_owner(owner)),
    );

    ctrl.reconcile("prod/web", &mut c, 0);

    let released = c.pods.get("prod/drifted").expect("pod still present");
    assert!(
        !released.meta().owner_references.iter().any(|r| r.controller && r.uid == rs.meta().uid),
        "the drifted pod was released (controller owner ref removed)"
    );
}

#[test]
fn missing_rs_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = ReplicaSetController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
}
