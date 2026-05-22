// SPDX-License-Identifier: Apache-2.0
//! `MemRegistry` conformance — exercises every Registry trait method.

use cave_home_cni_flannel::config::{BackendConfig, NetworkConfig, VxlanBackendConfig};
use cave_home_cni_flannel::subnet::lease::EventType;
use cave_home_cni_flannel::subnet::{LeaseAttrs, MockClock, Registry};
use cave_home_cni_flannel::subnet::mem_registry::MemRegistry;
use ipnet::Ipv4Net;
use std::str::FromStr;

fn cfg() -> NetworkConfig {
    NetworkConfig {
        network: Ipv4Net::from_str("10.244.0.0/16").unwrap(),
        subnet_len: 24,
        subnet_min: None,
        subnet_max: None,
        enable_ipv4: true,
        enable_ipv6: false,
        backend: BackendConfig::Vxlan(VxlanBackendConfig::default()),
    }
}

fn attrs(ip: &str) -> LeaseAttrs {
    LeaseAttrs {
        public_ip: ip.parse().unwrap(),
        backend_type: "vxlan".into(),
        backend_data: None,
    }
}

#[tokio::test]
async fn missing_network_config_is_an_error() {
    let r = MemRegistry::new(MockClock::new(0));
    assert!(r.get_network_config().await.is_err());
}

#[tokio::test]
async fn put_then_get_network_config_round_trips() {
    let r = MemRegistry::new(MockClock::new(0));
    r.put_network_config(&cfg()).await.unwrap();
    let got = r.get_network_config().await.unwrap();
    assert_eq!(got, cfg());
}

#[tokio::test]
async fn create_subnet_emits_added_event() {
    let r = MemRegistry::new(MockClock::new(0)).with_config(cfg());
    let mut rx = r.watch_subnets().await.unwrap();
    let n: Ipv4Net = "10.244.1.0/24".parse().unwrap();
    let _ = r.create_subnet(n, &attrs("10.0.0.1"), 60).await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.event_type, EventType::Added);
}

#[tokio::test]
async fn delete_subnet_emits_removed_event() {
    let r = MemRegistry::new(MockClock::new(0)).with_config(cfg());
    let n: Ipv4Net = "10.244.1.0/24".parse().unwrap();
    let _ = r.create_subnet(n, &attrs("10.0.0.1"), 60).await.unwrap();
    let mut rx = r.watch_subnets().await.unwrap();
    // Drain the replay event.
    let _ = rx.recv().await.unwrap();
    r.delete_subnet(n).await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.event_type, EventType::Removed);
}

#[tokio::test]
async fn create_conflict_when_different_public_ip() {
    let r = MemRegistry::new(MockClock::new(0)).with_config(cfg());
    let n: Ipv4Net = "10.244.1.0/24".parse().unwrap();
    let _ = r.create_subnet(n, &attrs("10.0.0.1"), 60).await.unwrap();
    let res = r.create_subnet(n, &attrs("10.0.0.2"), 60).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn update_missing_subnet_is_lease_not_found() {
    let r = MemRegistry::new(MockClock::new(0)).with_config(cfg());
    let n: Ipv4Net = "10.244.99.0/24".parse().unwrap();
    let res = r.update_subnet(n, &attrs("10.0.0.1"), 60).await;
    assert!(res.is_err());
}
