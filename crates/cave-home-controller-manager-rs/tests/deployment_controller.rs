// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the Deployment controller, which manages
//! `ReplicaSet`s (`pkg/controller/deployment` behavioural contract).

use cave_home_controller_manager_rs::apis::{
    template_hash, Cluster, Deployment, DeploymentSpec, DeploymentStrategy, PodTemplateSpec,
    ReplicaSet, ReplicaSetSpec,
};
use cave_home_controller_manager_rs::controllers::deployment::{DeploymentController, POD_TEMPLATE_HASH};
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::{Object, ObjectMeta, OwnerReference};

fn sel(app: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), app.to_owned());
    m
}

fn deploy(name: &str, replicas: i32, ver: &str, strategy: DeploymentStrategy) -> Deployment {
    Deployment::new(
        ObjectMeta::new(name, "prod", ""),
        DeploymentSpec {
            replicas,
            selector: sel("web"),
            template: PodTemplateSpec::with_labels(&[("app", "web"), ("ver", ver)]),
            strategy,
        },
    )
}

#[test]
fn fresh_deployment_creates_a_new_replicaset_at_desired() {
    let mut c = Cluster::new();
    let d = c.deployments.create(deploy("web", 3, "v1", DeploymentStrategy::default()));
    let mut ctrl = DeploymentController::new();

    assert_eq!(ctrl.reconcile("prod/web", &mut c, 0), Outcome::Done);

    let owned = c.replicasets.list_owned_by(&d.meta().uid);
    assert_eq!(owned.len(), 1, "exactly one new RS");
    let rs = &owned[0];
    let hash = template_hash(&d.spec.template);
    assert_eq!(rs.meta().name, format!("web-{hash}"), "RS named deploy-<hash>");
    assert_eq!(rs.spec.selector.get(POD_TEMPLATE_HASH), Some(&hash), "selector pins the hash");
    assert_eq!(rs.spec.template.labels.get(POD_TEMPLATE_HASH), Some(&hash), "template carries hash");
    assert_eq!(rs.spec.replicas, 3, "fresh create scales straight to desired");
}

#[test]
fn steady_state_is_a_noop() {
    let mut c = Cluster::new();
    c.deployments.create(deploy("web", 2, "v1", DeploymentStrategy::default()));
    let mut ctrl = DeploymentController::new();
    ctrl.reconcile("prod/web", &mut c, 0);
    let before: Vec<_> = c.replicasets.list().iter().map(|r| (r.meta().uid.clone(), r.spec.replicas)).collect();
    assert_eq!(ctrl.reconcile("prod/web", &mut c, 1), Outcome::Done);
    let after: Vec<_> = c.replicasets.list().iter().map(|r| (r.meta().uid.clone(), r.spec.replicas)).collect();
    assert_eq!(before, after, "no churn at steady state");
}

/// Helper: seed an "old" RS owned by the deployment, carrying a stale hash.
fn seed_old_rs(c: &mut Cluster, d: &Deployment, replicas: i32, available: i32) -> ReplicaSet {
    let stale = "oldhash";
    let mut selector = d.spec.selector.clone();
    selector.insert(POD_TEMPLATE_HASH.to_owned(), stale.to_owned());
    let mut meta = ObjectMeta::new(&format!("web-{stale}"), "prod", "");
    meta.labels.insert("app".to_owned(), "web".to_owned());
    meta.labels.insert(POD_TEMPLATE_HASH.to_owned(), stale.to_owned());
    meta.owner_references = vec![OwnerReference::to("Deployment", &d.meta().name, &d.meta().uid)
        .controller()
        .blocking()];
    let mut rs = ReplicaSet::new(meta, ReplicaSetSpec { replicas, selector, template: PodTemplateSpec::default() });
    rs.status.replicas = replicas;
    rs.status.ready_replicas = available;
    rs.status.available_replicas = available;
    c.replicasets.create(rs)
}

#[test]
fn rolling_update_surges_new_rs_before_cutting_old() {
    let mut c = Cluster::new();
    let d = c.deployments.create(deploy("web", 3, "v2", DeploymentStrategy::RollingUpdate {
        max_unavailable: 1,
        max_surge: 1,
    }));
    let old = seed_old_rs(&mut c, &d, 3, 3); // fully available old revision
    let mut ctrl = DeploymentController::new();

    ctrl.reconcile("prod/web", &mut c, 0);

    let hash = template_hash(&d.spec.template);
    let new_rs = c.replicasets.get(&format!("prod/web-{hash}")).expect("new RS created");
    assert_eq!(new_rs.spec.replicas, 1, "surge up new by maxSurge (3+1 total cap, old still 3)");
    let old_now = c.replicasets.get(&old.key()).expect("old RS present");
    assert_eq!(old_now.spec.replicas, 3, "old not yet cut — new pods not available yet");
}

#[test]
fn rolling_update_cuts_old_once_new_is_available() {
    let mut c = Cluster::new();
    let d = c.deployments.create(deploy("web", 3, "v2", DeploymentStrategy::RollingUpdate {
        max_unavailable: 1,
        max_surge: 1,
    }));
    let old = seed_old_rs(&mut c, &d, 3, 3);
    let mut ctrl = DeploymentController::new();
    ctrl.reconcile("prod/web", &mut c, 0); // new -> 1

    // Simulate the new RS's pod becoming available.
    let hash = template_hash(&d.spec.template);
    let mut new_rs = c.replicasets.get(&format!("prod/web-{hash}")).unwrap();
    new_rs.status.replicas = 1;
    new_rs.status.ready_replicas = 1;
    new_rs.status.available_replicas = 1;
    c.replicasets.update(new_rs);

    ctrl.reconcile("prod/web", &mut c, 1); // now old can be cut

    let old_now = c.replicasets.get(&old.key()).expect("old present");
    assert!(old_now.spec.replicas < 3, "old scaled down once surge capacity is available");
}

#[test]
fn recreate_scales_old_to_zero_then_brings_up_new() {
    let mut c = Cluster::new();
    let d = c.deployments.create(deploy("web", 3, "v2", DeploymentStrategy::Recreate));
    let old = seed_old_rs(&mut c, &d, 3, 3);
    let mut ctrl = DeploymentController::new();

    ctrl.reconcile("prod/web", &mut c, 0);
    let hash = template_hash(&d.spec.template);
    let new_rs = c.replicasets.get(&format!("prod/web-{hash}")).expect("new RS exists");
    assert_eq!(new_rs.spec.replicas, 0, "new held at 0 while old pods remain");
    assert_eq!(c.replicasets.get(&old.key()).unwrap().spec.replicas, 0, "old scaled to 0 first");

    // Old pods drain.
    let mut drained = c.replicasets.get(&old.key()).unwrap();
    drained.status.replicas = 0;
    c.replicasets.update(drained);

    ctrl.reconcile("prod/web", &mut c, 1);
    let new_rs = c.replicasets.get(&format!("prod/web-{hash}")).unwrap();
    assert_eq!(new_rs.spec.replicas, 3, "new scaled to desired once old is gone");
}

#[test]
fn status_aggregates_owned_replicasets() {
    let mut c = Cluster::new();
    c.deployments.create(deploy("web", 2, "v1", DeploymentStrategy::default()));
    let mut ctrl = DeploymentController::new();
    ctrl.reconcile("prod/web", &mut c, 0);
    // Give the new RS some observed status, reconcile to roll it up.
    let hash = template_hash(&c.deployments.get("prod/web").unwrap().spec.template);
    let mut rs = c.replicasets.get(&format!("prod/web-{hash}")).unwrap();
    rs.status.replicas = 2;
    rs.status.ready_replicas = 2;
    rs.status.available_replicas = 2;
    c.replicasets.update(rs);
    ctrl.reconcile("prod/web", &mut c, 1);

    let d = c.deployments.get("prod/web").unwrap();
    assert_eq!(d.status.replicas, 2);
    assert_eq!(d.status.ready_replicas, 2);
    assert_eq!(d.status.available_replicas, 2);
}

#[test]
fn missing_deployment_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = DeploymentController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
}
