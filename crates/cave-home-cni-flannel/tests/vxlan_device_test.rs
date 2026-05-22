// SPDX-License-Identifier: Apache-2.0
//! VXLAN device tests — Linux-only because they touch netlink.
//!
//! These tests are NOT `#[ignore]`-d; they're `#[cfg(target_os = "linux")]`
//! so non-Linux machines simply don't compile them. On a Linux box without
//! `CAP_NET_ADMIN`, the netlink calls return EPERM and the test asserts the
//! error path (matches manifest entry `vxlan-device-real-kernel`).

#![cfg(target_os = "linux")]

use cave_home_cni_flannel::backend::vxlan::config::VxlanDeviceAttrs;
use cave_home_cni_flannel::backend::vxlan::device::parse_mac;
use cave_home_cni_flannel::config::VxlanBackendConfig;
use ipnet::Ipv4Net;
use std::net::Ipv4Addr;
use std::str::FromStr;

#[test]
fn mac_round_trips_lowercase() {
    let m = parse_mac("0a:1b:2c:3d:4e:5f").expect("ok");
    assert_eq!(m, [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]);
}

#[test]
fn mac_round_trips_uppercase() {
    let m = parse_mac("AA:BB:CC:DD:EE:FF").expect("ok");
    assert_eq!(m, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
}

#[test]
fn mac_rejects_too_few_octets() {
    assert!(parse_mac("aa:bb:cc:dd:ee").is_err());
}

#[test]
fn mac_rejects_non_hex() {
    assert!(parse_mac("zz:zz:zz:zz:zz:zz").is_err());
}

#[test]
fn vxlan_device_attrs_compute_overhead() {
    let cfg = VxlanBackendConfig::default();
    let attrs = VxlanDeviceAttrs::from_config(
        &cfg,
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Net::from_str("10.244.5.0/24").unwrap(),
        1500,
    );
    assert_eq!(attrs.name, "flannel.1");
    assert_eq!(attrs.port, 8472);
    assert_eq!(attrs.mtu, 1450); // 1500 - 50
    assert_eq!(attrs.addr.prefix_len(), 32);
}

#[test]
fn vxlan_device_attrs_custom_vni() {
    let mut cfg = VxlanBackendConfig::default();
    cfg.vni = 4096;
    let attrs = VxlanDeviceAttrs::from_config(
        &cfg,
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Net::from_str("10.244.5.0/24").unwrap(),
        9000,
    );
    assert_eq!(attrs.name, "flannel.4096");
    assert_eq!(attrs.vni, 4096);
    assert_eq!(attrs.mtu, 8950);
}
