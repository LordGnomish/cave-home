// SPDX-License-Identifier: Apache-2.0
//! Subnet manager — port of `pkg/subnet/local_manager_test.go`.

use cave_home_cni_flannel::config::{BackendConfig, NetworkConfig, VxlanBackendConfig};
use cave_home_cni_flannel::subnet::{
    Clock, LeaseAttrs, LocalManager, MockClock, Registry, Reservation, SubnetError, SubnetManager,
};
use cave_home_cni_flannel::subnet::manager::DEFAULT_LEASE_TTL_SECS;
use cave_home_cni_flannel::subnet::mem_registry::MemRegistry;
use cave_home_cni_flannel::subnet::lease::EventType;
use ipnet::Ipv4Net;
use std::collections::HashSet;
use std::net::Ipv4Addr;
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

fn attrs(ip: &str) -> LeaseAttrs {
    LeaseAttrs {
        public_ip: ip.parse().unwrap(),
        backend_type: "vxlan".into(),
        backend_data: Some(serde_json::json!({"VtepMAC":"aa:bb:cc:dd:ee:ff"})),
    }
}

async fn boot() -> (
    Arc<MemRegistry<MockClock>>,
    Arc<MockClock>,
    LocalManager<MemRegistry<MockClock>, MockClock>,
) {
    let clock = Arc::new(MockClock::new(1_000));
    let reg = Arc::new(MemRegistry::new(MockClock::new(1_000)).with_config(cfg()));
    let mgr = LocalManager::new(reg.clone(), clock.clone());
    (reg, clock, mgr)
}

#[tokio::test]
async fn enumerate_candidates_step_size_correct() {
    let mut c = cfg();
    c.subnet_len = 24;
    let cands = LocalManager::<MemRegistry<MockClock>, MockClock>::enumerate_candidates(&c).unwrap();
    // 10.244.0.0/16 carved into /24 → 256 candidates.
    assert_eq!(cands.len(), 256);
    assert_eq!(cands[0], "10.244.0.0/24".parse::<Ipv4Net>().unwrap());
    assert_eq!(cands[255], "10.244.255.0/24".parse::<Ipv4Net>().unwrap());
}

#[tokio::test]
async fn enumerate_rejects_subnet_len_too_small() {
    let mut c = cfg();
    c.subnet_len = 16; // not strictly greater than network prefix
    let err = LocalManager::<MemRegistry<MockClock>, MockClock>::enumerate_candidates(&c).unwrap_err();
    assert!(matches!(err, SubnetError::InvalidConfig(_)));
}

#[tokio::test]
async fn enumerate_rejects_ipv4_disabled() {
    let mut c = cfg();
    c.enable_ipv4 = false;
    let err = LocalManager::<MemRegistry<MockClock>, MockClock>::enumerate_candidates(&c).unwrap_err();
    assert!(matches!(err, SubnetError::InvalidConfig(_)));
}

#[tokio::test]
async fn choose_subnet_skips_in_use() {
    let c = cfg();
    let in_use: HashSet<Ipv4Net> = ["10.244.0.0/24", "10.244.1.0/24"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    let mut rng = rand::thread_rng();
    let chosen =
        LocalManager::<MemRegistry<MockClock>, MockClock>::choose_subnet(&c, &in_use, &[], &mut rng)
            .unwrap();
    assert!(!in_use.contains(&chosen));
}

#[tokio::test]
async fn choose_subnet_skips_reservations() {
    let c = cfg();
    let in_use = HashSet::new();
    let res = vec![Reservation {
        subnet: "10.244.5.0/24".parse().unwrap(),
        public_ip: Ipv4Addr::new(10, 0, 0, 99),
    }];
    let mut rng = rand::thread_rng();
    for _ in 0..20 {
        let chosen = LocalManager::<MemRegistry<MockClock>, MockClock>::choose_subnet(
            &c, &in_use, &res, &mut rng,
        )
        .unwrap();
        assert_ne!(chosen, "10.244.5.0/24".parse::<Ipv4Net>().unwrap());
    }
}

#[tokio::test]
async fn choose_subnet_exhaustion() {
    // Tiny /30 network, /30 subnet len. Only 1 candidate; mark it in_use.
    let c = NetworkConfig {
        network: Ipv4Net::from_str("10.0.0.0/29").unwrap(),
        subnet_len: 30,
        subnet_min: None,
        subnet_max: None,
        enable_ipv4: true,
        enable_ipv6: false,
        backend: BackendConfig::default(),
    };
    let cands = LocalManager::<MemRegistry<MockClock>, MockClock>::enumerate_candidates(&c).unwrap();
    let in_use: HashSet<Ipv4Net> = cands.iter().copied().collect();
    let mut rng = rand::thread_rng();
    let err = LocalManager::<MemRegistry<MockClock>, MockClock>::choose_subnet(
        &c, &in_use, &[], &mut rng,
    )
    .unwrap_err();
    assert!(matches!(err, SubnetError::SubnetExhausted));
}

#[tokio::test]
async fn acquire_lease_persists_to_registry() {
    let (reg, _clock, mgr) = boot().await;
    let lease = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    let listed = reg.get_subnets().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].subnet, lease.subnet);
}

#[tokio::test]
async fn acquire_lease_idempotent_for_same_public_ip() {
    let (_reg, _clock, mgr) = boot().await;
    let l1 = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    let l2 = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    assert_eq!(l1.subnet, l2.subnet);
}

#[tokio::test]
async fn acquire_lease_distinct_for_distinct_ips() {
    let (_reg, _clock, mgr) = boot().await;
    let l1 = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    let l2 = mgr.acquire_lease(&attrs("10.0.0.2")).await.unwrap();
    assert_ne!(l1.subnet, l2.subnet);
}

#[tokio::test]
async fn renew_lease_extends_expiration() {
    let (_reg, clock, mgr) = boot().await;
    let l = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    let initial_exp = l.expiration;
    clock.advance(60);
    let l2 = mgr.renew_lease(l.subnet, &attrs("10.0.0.1")).await.unwrap();
    assert!(l2.expiration >= initial_exp);
}

#[tokio::test]
async fn revoke_lease_removes_from_registry() {
    let (reg, _clock, mgr) = boot().await;
    let l = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    mgr.revoke_lease(l.subnet).await.unwrap();
    assert!(reg.get_subnets().await.unwrap().is_empty());
}

#[tokio::test]
async fn watch_leases_replays_existing() {
    let (_reg, _clock, mgr) = boot().await;
    let l = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    let mut rx = mgr.watch_leases().await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.event_type, EventType::Added);
    assert_eq!(ev.lease.subnet, l.subnet);
}

#[tokio::test]
async fn watch_leases_streams_new_acquisitions() {
    let (_reg, _clock, mgr) = boot().await;
    let mut rx = mgr.watch_leases().await.unwrap();
    let l = mgr.acquire_lease(&attrs("10.0.0.5")).await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.lease.subnet, l.subnet);
}

#[tokio::test]
async fn reservation_pins_subnet() {
    let clock = Arc::new(MockClock::new(1_000));
    let reg = Arc::new(MemRegistry::new(MockClock::new(1_000)).with_config(cfg()));
    let pinned: Ipv4Net = "10.244.42.0/24".parse().unwrap();
    let mgr = LocalManager::new(reg.clone(), clock).with_reservations(vec![Reservation {
        subnet: pinned,
        public_ip: Ipv4Addr::new(10, 0, 0, 7),
    }]);
    let l = mgr.acquire_lease(&attrs("10.0.0.7")).await.unwrap();
    assert_eq!(l.subnet, pinned);
}

#[tokio::test]
async fn default_ttl_is_24h() {
    let (_reg, clock, mgr) = boot().await;
    let l = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    assert_eq!(l.expiration - clock.now(), DEFAULT_LEASE_TTL_SECS);
}

#[tokio::test]
async fn custom_ttl_honoured() {
    let clock = Arc::new(MockClock::new(0));
    let reg = Arc::new(MemRegistry::new(MockClock::new(0)).with_config(cfg()));
    let mgr = LocalManager::new(reg, clock.clone()).with_ttl(60);
    let l = mgr.acquire_lease(&attrs("10.0.0.1")).await.unwrap();
    assert_eq!(l.expiration, 60);
}
