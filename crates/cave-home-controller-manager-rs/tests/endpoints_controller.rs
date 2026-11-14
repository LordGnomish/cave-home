// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the Endpoints controller — populates an Endpoints
//! object from a Service's ready pods (`pkg/controller/endpoint`).

use cave_home_controller_manager_rs::apis::{Cluster, Pod, PodPhase, Service};
use cave_home_controller_manager_rs::controllers::endpoints::EndpointsController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::ObjectMeta;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn sel() -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), "web".to_owned());
    m
}

fn ready_pod(name: &str) -> Pod {
    let mut p = Pod::new(ObjectMeta::new(name, "prod", "").with_label("app", "web"));
    p.status.phase = PodPhase::Running;
    p.status.ready = true;
    p
}

#[test]
fn endpoints_collect_ready_matching_pods() -> TestResult {
    let mut c = Cluster::new();
    c.services
        .create(Service::new(ObjectMeta::new("web", "prod", ""), sel()));
    c.pods.create(ready_pod("a"));
    c.pods.create(ready_pod("b"));
    // Not ready -> excluded.
    let mut nr = ready_pod("c");
    nr.status.ready = false;
    c.pods.update(nr);
    // Wrong label -> excluded.
    c.pods.create(Pod::new(
        ObjectMeta::new("d", "prod", "").with_label("app", "db"),
    ));
    let mut ctrl = EndpointsController::new();

    assert_eq!(ctrl.reconcile("prod/web", &mut c, 0), Outcome::Done);
    let ep = c
        .endpoints
        .get("prod/web")
        .ok_or("endpoints object created with the service name")?;
    assert_eq!(
        ep.addresses,
        vec!["prod/a", "prod/b"],
        "only ready, matching pods, sorted"
    );
    Ok(())
}

#[test]
fn endpoints_update_when_a_pod_becomes_unready() -> TestResult {
    let mut c = Cluster::new();
    c.services
        .create(Service::new(ObjectMeta::new("web", "prod", ""), sel()));
    c.pods.create(ready_pod("a"));
    c.pods.create(ready_pod("b"));
    let mut ctrl = EndpointsController::new();
    ctrl.reconcile("prod/web", &mut c, 0);
    assert_eq!(
        c.endpoints
            .get("prod/web")
            .ok_or("expected Some, got None")?
            .addresses
            .len(),
        2
    );

    let mut b = c.pods.get("prod/b").ok_or("expected Some, got None")?;
    b.status.ready = false;
    c.pods.update(b);
    ctrl.reconcile("prod/web", &mut c, 1);
    assert_eq!(
        c.endpoints
            .get("prod/web")
            .ok_or("expected Some, got None")?
            .addresses,
        vec!["prod/a"],
        "unready pod dropped"
    );
    Ok(())
}

#[test]
fn service_with_no_ready_pods_has_empty_endpoints() -> TestResult {
    let mut c = Cluster::new();
    c.services
        .create(Service::new(ObjectMeta::new("web", "prod", ""), sel()));
    let mut ctrl = EndpointsController::new();
    ctrl.reconcile("prod/web", &mut c, 0);
    assert_eq!(
        c.endpoints
            .get("prod/web")
            .ok_or("expected Some, got None")?
            .addresses,
        [] as [String; 0]
    );
    Ok(())
}

#[test]
fn missing_service_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = EndpointsController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
    assert!(c.endpoints.get("prod/ghost").is_none());
}
