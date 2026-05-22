// SPDX-License-Identifier: Apache-2.0
//! ServiceCache tests — upstream `pkg/proxy/serviceconfig.go` +
//! `pkg/proxy/servicechangetracker.go` event-driven semantics.

use cave_home_kube_proxy_rs::api::{
    NamespacedName, Protocol, Service, ServicePort, ServiceType,
};
use cave_home_kube_proxy_rs::cache::service_cache::ServiceCache;

fn make_svc(ns: &str, name: &str, ip: &str) -> Service {
    Service {
        metadata: NamespacedName::new(ns, name),
        cluster_ip: ip.into(),
        ports: vec![ServicePort { name: "p80".into(), port: 80, protocol: Protocol::Tcp }],
        type_: ServiceType::ClusterIP,
    }
}

#[test]
fn empty_cache_yields_empty_snapshot() {
    let cache = ServiceCache::new();
    let snap = cache.snapshot();
    assert!(snap.is_empty());
}

#[test]
fn add_inserts_service_into_snapshot() {
    let cache = ServiceCache::new();
    let svc = make_svc("ns1", "svc1", "10.20.30.41");
    cache.add(svc.clone());
    let snap = cache.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].name.namespaced_name.to_string(), "ns1/svc1");
    assert_eq!(snap[0].cluster_ip, "10.20.30.41");
}

#[test]
fn modify_replaces_existing_entry() {
    let cache = ServiceCache::new();
    cache.add(make_svc("ns1", "svc1", "10.0.0.1"));
    cache.modify(make_svc("ns1", "svc1", "10.0.0.99"));
    let snap = cache.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].cluster_ip, "10.0.0.99");
}

#[test]
fn delete_removes_entry() {
    let cache = ServiceCache::new();
    let svc = make_svc("ns1", "svc1", "10.0.0.1");
    cache.add(svc.clone());
    cache.delete(&svc);
    assert!(cache.snapshot().is_empty());
}

#[test]
fn skipped_services_do_not_appear_in_snapshot() {
    // ExternalName / empty ClusterIP must be filtered out (upstream
    // ShouldSkipService).
    let cache = ServiceCache::new();
    let mut svc = make_svc("ns1", "external", "");
    svc.type_ = ServiceType::ExternalName;
    cache.add(svc);
    cache.add(make_svc("ns1", "skip-none", "None"));
    cache.add(make_svc("ns1", "real", "10.20.30.41"));
    let snap = cache.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].name.namespaced_name.name, "real");
}

#[test]
fn snapshot_explodes_multiport_services_to_one_per_port() {
    let cache = ServiceCache::new();
    let svc = Service {
        metadata: NamespacedName::new("ns1", "multi"),
        cluster_ip: "10.0.0.1".into(),
        ports: vec![
            ServicePort { name: "p80".into(), port: 80, protocol: Protocol::Tcp },
            ServicePort { name: "p443".into(), port: 443, protocol: Protocol::Tcp },
        ],
        type_: ServiceType::ClusterIP,
    };
    cache.add(svc);
    let snap = cache.snapshot();
    assert_eq!(snap.len(), 2);
    let ports: Vec<i32> = snap.iter().map(|s| s.port).collect();
    assert!(ports.contains(&80));
    assert!(ports.contains(&443));
}

#[test]
fn dirty_flag_set_on_change_cleared_on_take() {
    let cache = ServiceCache::new();
    assert!(!cache.is_dirty());
    cache.add(make_svc("ns1", "svc1", "10.0.0.1"));
    assert!(cache.is_dirty());
    let _ = cache.take_dirty();
    assert!(!cache.is_dirty());
}

#[test]
fn snapshot_is_sorted_by_service_port_name() {
    let cache = ServiceCache::new();
    cache.add(make_svc("ns2", "z-svc", "10.0.0.2"));
    cache.add(make_svc("ns1", "a-svc", "10.0.0.1"));
    let snap = cache.snapshot();
    assert_eq!(snap[0].name.namespaced_name.namespace, "ns1");
    assert_eq!(snap[1].name.namespaced_name.namespace, "ns2");
}
