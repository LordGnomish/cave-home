// SPDX-License-Identifier: Apache-2.0
//! End-to-end: a Deployment drives ReplicaSet creation, which drives Pod
//! creation, all the way to a converged, fully-available rollout — through the
//! work queue + resync manager, with kubelet pod-admission simulated.

use cave_home_controller_manager_rs::apis::{
    template_hash, Deployment, DeploymentSpec, DeploymentStrategy, PodPhase, PodTemplateSpec,
};
use cave_home_controller_manager_rs::manager::Manager;
use cave_home_controller_manager_rs::types::{Object, ObjectMeta};

fn sel(app: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), app.to_owned());
    m
}

fn deploy(replicas: i32, ver: &str) -> Deployment {
    Deployment::new(
        ObjectMeta::new("web", "prod", ""),
        DeploymentSpec {
            replicas,
            selector: sel("web"),
            template: PodTemplateSpec::with_labels(&[("app", "web"), ("ver", ver)]),
            strategy: DeploymentStrategy::RollingUpdate { max_unavailable: 1, max_surge: 1 },
        },
    )
}

fn ready_pods(m: &Manager) -> usize {
    m.cluster
        .pods
        .list()
        .iter()
        .filter(|p| p.status.phase == PodPhase::Running && p.status.ready)
        .count()
}

#[test]
fn deployment_creates_replicaset_creates_pods_to_convergence() {
    let mut m = Manager::new();
    let d = m.cluster.deployments.create(deploy(3, "v1"));

    let rounds = m.run_until_stable(0, 50);
    assert!(rounds < 50, "the rollout converged (took {rounds} rounds)");

    // One ReplicaSet, owned by the Deployment, at desired replicas + available.
    let rses = m.cluster.replicasets.list_owned_by(&d.meta().uid);
    assert_eq!(rses.len(), 1, "exactly one RS");
    assert_eq!(rses[0].spec.replicas, 3);
    assert_eq!(rses[0].status.available_replicas, 3, "RS reports all pods available");

    // Three pods, owned by the RS, all running+ready.
    let pods = m.cluster.pods.list_owned_by(&rses[0].meta().uid);
    assert_eq!(pods.len(), 3, "three pods exist");
    assert_eq!(ready_pods(&m), 3, "all pods admitted and ready");

    // Deployment status reflects the converged rollout.
    let d2 = m.cluster.deployments.get("prod/web").unwrap();
    assert_eq!(d2.status.replicas, 3);
    assert_eq!(d2.status.available_replicas, 3);
    assert_eq!(d2.status.updated_replicas, 3);
}

#[test]
fn scaling_the_deployment_up_and_down_converges() {
    let mut m = Manager::new();
    m.cluster.deployments.create(deploy(2, "v1"));
    m.run_until_stable(0, 50);
    assert_eq!(ready_pods(&m), 2);

    // Scale up to 5.
    let mut d = m.cluster.deployments.get("prod/web").unwrap();
    d.spec.replicas = 5;
    m.cluster.deployments.update(d);
    m.run_until_stable(1, 50);
    assert_eq!(ready_pods(&m), 5, "scaled up to 5");

    // Scale down to 1.
    let mut d = m.cluster.deployments.get("prod/web").unwrap();
    d.spec.replicas = 1;
    m.cluster.deployments.update(d);
    m.run_until_stable(2, 50);
    assert_eq!(m.cluster.pods.list().iter().filter(|p| p.is_active()).count(), 1, "scaled down to 1");
}

#[test]
fn rolling_update_replaces_pods_with_the_new_revision() {
    let mut m = Manager::new();
    let d = m.cluster.deployments.create(deploy(3, "v1"));
    m.run_until_stable(0, 50);
    let v1_hash = template_hash(&d.spec.template);

    // Roll out a new template revision.
    let mut d2 = m.cluster.deployments.get("prod/web").unwrap();
    d2.spec.template = PodTemplateSpec::with_labels(&[("app", "web"), ("ver", "v2")]);
    let v2_hash = template_hash(&d2.spec.template);
    assert_ne!(v1_hash, v2_hash, "template change yields a new hash");
    m.cluster.deployments.update(d2);

    let rounds = m.run_until_stable(10, 100);
    assert!(rounds < 100, "rollout converged in {rounds} rounds");

    // The new RS is at desired; the old RS is scaled to zero.
    let new_rs = m.cluster.replicasets.get(&format!("prod/web-{v2_hash}")).expect("new RS exists");
    assert_eq!(new_rs.spec.replicas, 3, "new revision at desired");
    let old_rs = m.cluster.replicasets.get(&format!("prod/web-{v1_hash}")).expect("old RS still tracked");
    assert_eq!(old_rs.spec.replicas, 0, "old revision drained to zero");

    // Total active pods equal desired, all on the new revision.
    let active: Vec<_> = m.cluster.pods.list().into_iter().filter(|p| p.is_active()).collect();
    assert_eq!(active.len(), 3, "exactly desired pods remain active");
    for p in &active {
        assert_eq!(p.meta().labels.get("ver").map(String::as_str), Some("v2"), "all pods on v2");
    }
}
