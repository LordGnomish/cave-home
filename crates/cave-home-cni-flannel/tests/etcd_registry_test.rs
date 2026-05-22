// SPDX-License-Identifier: Apache-2.0
//! Etcd-registry conformance.
//!
//! Live etcd integration is opt-in (set `CAVE_ETCD_ENDPOINT=http://127.0.0.1:2379`).
//! By default we exercise the same `Registry` trait surface against
//! `MemRegistry` so the contract conformance is enforced everywhere — the
//! parity manifest records this honestly under [[unmapped]] `etcd-live-test`.

use cave_home_cni_flannel::config::{BackendConfig, NetworkConfig, VxlanBackendConfig};
use cave_home_cni_flannel::subnet::lease::EventType;
use cave_home_cni_flannel::subnet::mem_registry::MemRegistry;
use cave_home_cni_flannel::subnet::{LeaseAttrs, MockClock, Registry};
use ipnet::Ipv4Net;
use std::str::FromStr;
use std::sync::Arc;

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

/// Each test runs the same body against an arbitrary `Arc<dyn Registry>` so
/// swapping in `EtcdRegistry::connect` requires zero diff once etcd is live.
async fn assert_contract(reg: Arc<dyn Registry>) {
    reg.put_network_config(&cfg()).await.unwrap();
    let got = reg.get_network_config().await.unwrap();
    assert_eq!(got, cfg());

    let n: Ipv4Net = "10.244.1.0/24".parse().unwrap();
    let attrs = LeaseAttrs {
        public_ip: "10.0.0.1".parse().unwrap(),
        backend_type: "vxlan".into(),
        backend_data: None,
    };
    let lease = reg.create_subnet(n, &attrs, 60).await.unwrap();
    assert_eq!(lease.subnet, n);

    let listed = reg.get_subnets().await.unwrap();
    assert!(listed.iter().any(|l| l.subnet == n));

    reg.delete_subnet(n).await.unwrap();
}

#[tokio::test]
async fn registry_contract_via_mem_registry() {
    let reg: Arc<dyn Registry> = Arc::new(MemRegistry::new(MockClock::new(0)));
    assert_contract(reg).await;
}

#[tokio::test]
async fn watch_emits_events_through_trait_object() {
    let reg: Arc<dyn Registry> = Arc::new(MemRegistry::new(MockClock::new(0)).with_config(cfg()));
    let mut rx = reg.watch_subnets().await.unwrap();
    let n: Ipv4Net = "10.244.7.0/24".parse().unwrap();
    let attrs = LeaseAttrs {
        public_ip: "10.0.0.7".parse().unwrap(),
        backend_type: "vxlan".into(),
        backend_data: None,
    };
    let _ = reg.create_subnet(n, &attrs, 60).await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.event_type, EventType::Added);
    assert_eq!(ev.lease.subnet, n);
}

#[tokio::test]
async fn ttl_zero_still_round_trips() {
    let reg: Arc<dyn Registry> = Arc::new(MemRegistry::new(MockClock::new(0)).with_config(cfg()));
    let n: Ipv4Net = "10.244.8.0/24".parse().unwrap();
    let attrs = LeaseAttrs {
        public_ip: "10.0.0.8".parse().unwrap(),
        backend_type: "vxlan".into(),
        backend_data: None,
    };
    let lease = reg.create_subnet(n, &attrs, 0).await.unwrap();
    assert_eq!(lease.subnet, n);
}

#[tokio::test]
async fn live_etcd_smoke_test_when_endpoint_set() {
    // Honest: this test ONLY runs if the operator sets CAVE_ETCD_ENDPOINT.
    // Otherwise it short-circuits — see parity.manifest.toml [[unmapped]]
    // `etcd-live-test`.
    let Ok(_endpoint) = std::env::var("CAVE_ETCD_ENDPOINT") else {
        eprintln!("CAVE_ETCD_ENDPOINT unset — skipping live etcd test");
        return;
    };
    #[cfg(target_os = "linux")]
    {
        let reg = cave_home_cni_flannel::subnet::etcd_registry::EtcdRegistry::connect(
            [_endpoint.as_str()],
            Some("/cave-home-test".into()),
        )
        .await;
        assert!(reg.is_ok(), "live etcd connect failed: {:?}", reg.err());
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("EtcdRegistry is Linux-gated — skipping on non-Linux");
    }
}

#[tokio::test]
async fn empty_get_subnets_is_ok() {
    let reg: Arc<dyn Registry> = Arc::new(MemRegistry::new(MockClock::new(0)).with_config(cfg()));
    let listed = reg.get_subnets().await.unwrap();
    assert!(listed.is_empty());
}

#[tokio::test]
async fn delete_unknown_subnet_is_idempotent() {
    let reg: Arc<dyn Registry> = Arc::new(MemRegistry::new(MockClock::new(0)).with_config(cfg()));
    let n: Ipv4Net = "10.244.111.0/24".parse().unwrap();
    reg.delete_subnet(n).await.unwrap(); // no-op
}
