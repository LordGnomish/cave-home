// SPDX-License-Identifier: Apache-2.0
//! EndpointSliceCache tests — upstream `pkg/proxy/endpointslicecache.go`.
//! Multiple slices may belong to the same service; the cache merges them
//! into one endpoint set per ServicePortName.

use cave_home_kube_proxy_rs::api::{
    Endpoint, EndpointConditions, EndpointPort, EndpointSlice, NamespacedName, Protocol,
    ServicePortName,
};
use cave_home_kube_proxy_rs::cache::endpointslice_cache::EndpointSliceCache;
use cave_home_kube_proxy_rs::cache::source::{EventSource, MockEventSource};
use cave_home_kube_proxy_rs::api::WatchEvent;

fn ready_ep(ip: &str) -> Endpoint {
    Endpoint {
        addresses: vec![ip.into()],
        conditions: EndpointConditions { ready: Some(true), serving: None, terminating: None },
    }
}

fn unready_ep(ip: &str) -> Endpoint {
    Endpoint {
        addresses: vec![ip.into()],
        conditions: EndpointConditions { ready: Some(false), serving: None, terminating: None },
    }
}

fn slice(ns: &str, slice_name: &str, svc: &str, eps: Vec<Endpoint>) -> EndpointSlice {
    EndpointSlice {
        metadata: NamespacedName::new(ns, slice_name),
        service_name: svc.into(),
        ports: vec![EndpointPort { name: "p80".into(), port: 80, protocol: Protocol::Tcp }],
        endpoints: eps,
    }
}

fn spn(ns: &str, name: &str, port: &str) -> ServicePortName {
    ServicePortName {
        namespaced_name: NamespacedName::new(ns, name),
        port: port.into(),
        protocol: Protocol::Tcp,
    }
}

#[test]
fn empty_cache_yields_empty_snapshot() {
    let cache = EndpointSliceCache::new();
    let snap = cache.snapshot();
    assert!(snap.is_empty());
}

#[test]
fn add_slice_makes_endpoints_visible_under_service_port_name() {
    let cache = EndpointSliceCache::new();
    cache.add(slice("ns1", "svc1-abc", "svc1", vec![ready_ep("10.180.0.1")]));
    let snap = cache.snapshot();
    let key = spn("ns1", "svc1", "p80");
    let eps = snap.get(&key).expect("svc1:p80 must be present");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].ip, "10.180.0.1");
    assert_eq!(eps[0].port, 80);
}

#[test]
fn merges_multiple_slices_for_same_service() {
    let cache = EndpointSliceCache::new();
    cache.add(slice("ns1", "svc1-shard-a", "svc1", vec![ready_ep("10.180.0.1")]));
    cache.add(slice("ns1", "svc1-shard-b", "svc1", vec![ready_ep("10.180.0.2")]));
    let snap = cache.snapshot();
    let eps = snap.get(&spn("ns1", "svc1", "p80")).unwrap();
    assert_eq!(eps.len(), 2);
}

#[test]
fn unready_endpoints_excluded() {
    let cache = EndpointSliceCache::new();
    cache.add(slice("ns1", "svc1-abc", "svc1",
        vec![ready_ep("10.180.0.1"), unready_ep("10.180.0.2")]));
    let snap = cache.snapshot();
    let eps = snap.get(&spn("ns1", "svc1", "p80")).unwrap();
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].ip, "10.180.0.1");
}

#[test]
fn modify_replaces_slice_endpoints() {
    let cache = EndpointSliceCache::new();
    cache.add(slice("ns1", "svc1-abc", "svc1", vec![ready_ep("10.180.0.1")]));
    cache.modify(slice("ns1", "svc1-abc", "svc1", vec![ready_ep("10.180.0.99")]));
    let eps = cache.snapshot().get(&spn("ns1", "svc1", "p80")).cloned().unwrap_or_default();
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].ip, "10.180.0.99");
}

#[test]
fn delete_slice_removes_its_endpoints() {
    let cache = EndpointSliceCache::new();
    cache.add(slice("ns1", "svc1-a", "svc1", vec![ready_ep("10.180.0.1")]));
    cache.add(slice("ns1", "svc1-b", "svc1", vec![ready_ep("10.180.0.2")]));
    let s = slice("ns1", "svc1-a", "svc1", vec![]);
    cache.delete(&s);
    let eps = cache.snapshot().get(&spn("ns1", "svc1", "p80")).cloned().unwrap_or_default();
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].ip, "10.180.0.2");
}

#[test]
fn snapshot_endpoints_sorted_by_ip_and_port() {
    let cache = EndpointSliceCache::new();
    cache.add(slice("ns1", "svc1-a", "svc1",
        vec![ready_ep("10.180.0.3"), ready_ep("10.180.0.1"), ready_ep("10.180.0.2")]));
    let eps = cache.snapshot().get(&spn("ns1", "svc1", "p80")).cloned().unwrap_or_default();
    let ips: Vec<&str> = eps.iter().map(|e| e.ip.as_str()).collect();
    assert_eq!(ips, vec!["10.180.0.1", "10.180.0.2", "10.180.0.3"]);
}

#[test]
fn dirty_flag_signals_pending_changes() {
    let cache = EndpointSliceCache::new();
    assert!(!cache.is_dirty());
    cache.add(slice("ns1", "x", "svc1", vec![ready_ep("10.180.0.1")]));
    assert!(cache.is_dirty());
    cache.take_dirty();
    assert!(!cache.is_dirty());
}

// --- EventSource trait + MockEventSource ----------------------------------

#[tokio::test]
async fn mock_event_source_streams_pre_loaded_events() {
    let src = MockEventSource::new();
    src.push(WatchEvent::EndpointSliceAdded(slice("ns1", "x", "svc1",
        vec![ready_ep("10.180.0.1")])));
    src.push(WatchEvent::EndpointSliceDeleted(slice("ns1", "x", "svc1", vec![])));
    src.close();

    let mut events = Vec::new();
    let mut stream = src.stream();
    while let Some(e) = stream.recv().await {
        events.push(e);
    }
    assert_eq!(events.len(), 2);
    matches!(events[0], WatchEvent::EndpointSliceAdded(_));
    matches!(events[1], WatchEvent::EndpointSliceDeleted(_));
}

#[tokio::test]
async fn mock_event_source_stream_closes_on_drop() {
    let src = MockEventSource::new();
    let mut stream = src.stream();
    src.close();
    // Stream returns None once closed.
    let next = stream.recv().await;
    assert!(next.is_none());
}
