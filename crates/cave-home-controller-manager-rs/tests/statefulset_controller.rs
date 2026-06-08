// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the StatefulSet controller — ordered, stable pod
//! identities (`pkg/controller/statefulset` contract).

use cave_home_controller_manager_rs::apis::{
    Cluster, PodManagementPolicy, PodPhase, PodTemplateSpec, StatefulSet, StatefulSetSpec,
};
use cave_home_controller_manager_rs::controllers::statefulset::StatefulSetController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::{Object, ObjectMeta};

fn sel() -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), "db".to_owned());
    m
}

fn sts(replicas: i32) -> StatefulSet {
    StatefulSet::new(
        ObjectMeta::new("db", "prod", ""),
        StatefulSetSpec {
            replicas,
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("app", "db")]),
            ..StatefulSetSpec::default()
        },
    )
}

fn admit(c: &mut Cluster, name: &str) {
    let mut p = c.pods.get(&format!("prod/{name}")).expect("pod exists");
    p.status.phase = PodPhase::Running;
    p.status.ready = true;
    c.pods.update(p);
}

fn pod_names(c: &Cluster) -> Vec<String> {
    let mut v: Vec<String> = c.pods.list().iter().map(|p| p.meta().name.clone()).collect();
    v.sort();
    v
}

#[test]
fn creates_the_first_ordinal_then_waits_for_readiness() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts(3));
    let mut ctrl = StatefulSetController::new();

    ctrl.reconcile("prod/db", &mut c, 0);
    assert_eq!(pod_names(&c), vec!["db-0"], "only ordinal 0 created first");

    // Ordinal 1 must not appear until 0 is ready.
    ctrl.reconcile("prod/db", &mut c, 1);
    assert_eq!(pod_names(&c), vec!["db-0"], "no progress while 0 is not ready");

    admit(&mut c, "db-0");
    ctrl.reconcile("prod/db", &mut c, 2);
    assert_eq!(pod_names(&c), vec!["db-0", "db-1"], "ordinal 1 created once 0 is ready");
}

#[test]
fn fills_all_ordinals_in_order_as_each_becomes_ready() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts(3));
    let mut ctrl = StatefulSetController::new();
    for i in 0..3 {
        ctrl.reconcile("prod/db", &mut c, i);
        admit(&mut c, &format!("db-{i}"));
    }
    ctrl.reconcile("prod/db", &mut c, 10);
    assert_eq!(pod_names(&c), vec!["db-0", "db-1", "db-2"]);
}

#[test]
fn scales_down_highest_ordinal_first() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts(3));
    let mut ctrl = StatefulSetController::new();
    for i in 0..3 {
        ctrl.reconcile("prod/db", &mut c, i);
        admit(&mut c, &format!("db-{i}"));
    }
    ctrl.reconcile("prod/db", &mut c, 10);
    assert_eq!(pod_names(&c).len(), 3);

    // Scale down to 1.
    let mut s = c.statefulsets.get("prod/db").unwrap();
    s.spec.replicas = 1;
    c.statefulsets.update(s);

    ctrl.reconcile("prod/db", &mut c, 11);
    assert_eq!(pod_names(&c), vec!["db-0", "db-1"], "highest ordinal (db-2) removed first");
    ctrl.reconcile("prod/db", &mut c, 12);
    assert_eq!(pod_names(&c), vec!["db-0"], "then db-1");
}

#[test]
fn steady_state_is_a_noop() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts(1));
    let mut ctrl = StatefulSetController::new();
    ctrl.reconcile("prod/db", &mut c, 0);
    admit(&mut c, "db-0");
    let before = pod_names(&c);
    assert_eq!(ctrl.reconcile("prod/db", &mut c, 1), Outcome::Done);
    assert_eq!(pod_names(&c), before);
}

#[test]
fn missing_statefulset_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = StatefulSetController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
}

// --- Status subresource -------------------------------------------------

#[test]
fn status_reports_replicas_and_ready_counts() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts(3));
    let mut ctrl = StatefulSetController::new();
    // Bring up all three ordinals.
    for i in 0..3 {
        ctrl.reconcile("prod/db", &mut c, i);
        admit(&mut c, &format!("db-{i}"));
    }
    ctrl.reconcile("prod/db", &mut c, 10);
    let st = c.statefulsets.get("prod/db").unwrap().status;
    assert_eq!(st.replicas, 3, "three pods created");
    assert_eq!(st.ready_replicas, 3, "all ready");
    assert_eq!(st.current_replicas, 3);
    assert_eq!(st.updated_replicas, 3);
}

// --- volumeClaimTemplates → PVCs ----------------------------------------

fn sts_with_pvc(replicas: i32, templates: &[&str]) -> StatefulSet {
    StatefulSet::new(
        ObjectMeta::new("db", "prod", ""),
        StatefulSetSpec {
            replicas,
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("app", "db")]),
            pod_management_policy: PodManagementPolicy::OrderedReady,
            volume_claim_templates: templates.iter().map(|s| (*s).to_owned()).collect(),
        },
    )
}

fn pvc_names(c: &Cluster) -> Vec<String> {
    let mut v: Vec<String> = c.pvcs.list().iter().map(|p| p.meta().name.clone()).collect();
    v.sort();
    v
}

#[test]
fn creates_one_pvc_per_template_per_ordinal() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts_with_pvc(2, &["data", "logs"]));
    let mut ctrl = StatefulSetController::new();
    // Ordinal 0 + its PVCs appear; the claim name is "<template>-<sts>-<ordinal>".
    ctrl.reconcile("prod/db", &mut c, 0);
    assert_eq!(pvc_names(&c), vec!["data-db-0", "logs-db-0"], "PVCs for ordinal 0");
    admit(&mut c, "db-0");
    ctrl.reconcile("prod/db", &mut c, 1);
    assert_eq!(
        pvc_names(&c),
        vec!["data-db-0", "data-db-1", "logs-db-0", "logs-db-1"],
        "PVCs for ordinal 1 added once it is created"
    );
}

#[test]
fn pvcs_survive_scale_down() {
    let mut c = Cluster::new();
    c.statefulsets.create(sts_with_pvc(2, &["data"]));
    let mut ctrl = StatefulSetController::new();
    ctrl.reconcile("prod/db", &mut c, 0);
    admit(&mut c, "db-0");
    ctrl.reconcile("prod/db", &mut c, 1);
    admit(&mut c, "db-1");
    assert_eq!(pvc_names(&c), vec!["data-db-0", "data-db-1"]);

    // Scale to 1: db-1 the pod is deleted, but its PVC is retained.
    let mut s = c.statefulsets.get("prod/db").unwrap();
    s.spec.replicas = 1;
    c.statefulsets.update(s);
    ctrl.reconcile("prod/db", &mut c, 2);
    assert_eq!(pod_names(&c), vec!["db-0"], "db-1 pod removed");
    assert_eq!(pvc_names(&c), vec!["data-db-0", "data-db-1"], "PVC for db-1 retained");
}

// --- podManagementPolicy: Parallel --------------------------------------

#[test]
fn parallel_policy_creates_all_ordinals_at_once() {
    let mut c = Cluster::new();
    c.statefulsets.create(StatefulSet::new(
        ObjectMeta::new("db", "prod", ""),
        StatefulSetSpec {
            replicas: 3,
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("app", "db")]),
            pod_management_policy: PodManagementPolicy::Parallel,
            volume_claim_templates: Vec::new(),
        },
    ));
    let mut ctrl = StatefulSetController::new();
    // No readiness gating: a single reconcile brings up all three ordinals.
    ctrl.reconcile("prod/db", &mut c, 0);
    assert_eq!(pod_names(&c), vec!["db-0", "db-1", "db-2"], "Parallel creates all at once");
}

#[test]
fn parallel_policy_deletes_all_surplus_at_once() {
    let mut c = Cluster::new();
    c.statefulsets.create(StatefulSet::new(
        ObjectMeta::new("db", "prod", ""),
        StatefulSetSpec {
            replicas: 3,
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("app", "db")]),
            pod_management_policy: PodManagementPolicy::Parallel,
            volume_claim_templates: Vec::new(),
        },
    ));
    let mut ctrl = StatefulSetController::new();
    ctrl.reconcile("prod/db", &mut c, 0);
    for i in 0..3 {
        admit(&mut c, &format!("db-{i}"));
    }
    let mut s = c.statefulsets.get("prod/db").unwrap();
    s.spec.replicas = 1;
    c.statefulsets.update(s);
    ctrl.reconcile("prod/db", &mut c, 1);
    assert_eq!(pod_names(&c), vec!["db-0"], "Parallel removes db-1 and db-2 in one pass");
}
