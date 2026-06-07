// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the in-memory apiserver (`apis::Cluster` + `Api<T>`).
//!
//! This is the test backbone the workload controllers reconcile against — the
//! behavioural analogue of client-go's `testing.ObjectTracker` / `fake.Clientset`
//! (a *real* in-memory implementation of the create/get/update/delete/list
//! contract, not a stub). The networked REST client remains deferred.

use cave_home_controller_manager_rs::apis::{
    Api, Cluster, Pod, PodPhase, PodTemplateSpec, ReplicaSet, ReplicaSetSpec,
};
use cave_home_controller_manager_rs::types::{Object, ObjectMeta, OwnerReference};

fn selector(app: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("app".to_owned(), app.to_owned());
    m
}

#[test]
fn create_assigns_a_unique_uid_when_absent() {
    let mut api: Api<Pod> = Api::new("pod");
    let first = api.create(Pod::new(ObjectMeta::new("web-a", "prod", "")));
    let second = api.create(Pod::new(ObjectMeta::new("web-b", "prod", "")));
    assert!(!first.meta().uid.is_empty(), "uid assigned on create");
    assert_ne!(first.meta().uid, second.meta().uid, "uids are unique");
    // The stored copy carries the assigned uid.
    assert_eq!(
        api.get("prod/web-a").map(|p| p.meta().uid.clone()),
        Some(first.meta().uid.clone())
    );
}

#[test]
fn create_preserves_a_caller_supplied_uid() {
    let mut api: Api<Pod> = Api::new("pod");
    let p = api.create(Pod::new(ObjectMeta::new("web", "prod", "fixed-uid")));
    assert_eq!(p.meta().uid, "fixed-uid");
}

#[test]
fn update_replaces_and_delete_removes() {
    let mut api: Api<Pod> = Api::new("pod");
    let mut p = api.create(Pod::new(ObjectMeta::new("web", "prod", "")));
    p.status.phase = PodPhase::Running;
    api.update(p);
    assert_eq!(api.get("prod/web").map(|p| p.status.phase), Some(PodPhase::Running));
    assert!(api.delete("prod/web").is_some());
    assert!(api.get("prod/web").is_none());
}

#[test]
fn list_matching_filters_by_namespace_and_selector() {
    let mut api: Api<Pod> = Api::new("pod");
    api.create(Pod::new(ObjectMeta::new("a", "prod", "").with_label("app", "web")));
    api.create(Pod::new(ObjectMeta::new("b", "prod", "").with_label("app", "db")));
    api.create(Pod::new(ObjectMeta::new("c", "stage", "").with_label("app", "web")));
    let web_prod = api.list_matching("prod", &selector("web"));
    let names: Vec<_> = web_prod.iter().map(|p| p.meta().name.clone()).collect();
    assert_eq!(names, vec!["a"], "namespace AND selector both applied");
}

#[test]
fn list_owned_by_returns_only_controller_owned_children() {
    let mut api: Api<Pod> = Api::new("pod");
    let owner = OwnerReference::to("ReplicaSet", "rs", "rs-uid").controller();
    api.create(Pod::new(ObjectMeta::new("owned", "prod", "").with_owner(owner)));
    // A pod that merely *references* rs-uid but not as controller is not owned.
    let noncontroller = OwnerReference::to("ReplicaSet", "rs", "rs-uid");
    api.create(Pod::new(ObjectMeta::new("ref-only", "prod", "").with_owner(noncontroller)));
    api.create(Pod::new(ObjectMeta::new("orphan", "prod", "")));
    let children = api.list_owned_by("rs-uid");
    let names: Vec<_> = children.iter().map(|p| p.meta().name.clone()).collect();
    assert_eq!(names, vec!["owned"], "only the controller-owned child is returned");
}

#[test]
fn cluster_bundles_typed_apis() {
    let mut c = Cluster::new();
    let rs = c.replicasets.create(ReplicaSet::new(
        ObjectMeta::new("web", "prod", ""),
        ReplicaSetSpec {
            replicas: 3,
            selector: selector("web"),
            template: PodTemplateSpec::with_labels(&[("app", "web")]),
        },
    ));
    assert!(!rs.meta().uid.is_empty());
    assert_eq!(c.replicasets.get("prod/web").map(|r| r.spec.replicas), Some(3));
    assert!(c.pods.list().is_empty(), "no pods until a controller creates them");
}
