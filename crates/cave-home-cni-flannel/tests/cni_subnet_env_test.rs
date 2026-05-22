// SPDX-License-Identifier: Apache-2.0
//! `subnet.env` parser tests.

use cave_home_cni_flannel::cni::parse_subnet_env;

const HAPPY: &str = "FLANNEL_NETWORK=10.244.0.0/16\n\
FLANNEL_SUBNET=10.244.7.1/24\n\
FLANNEL_MTU=1450\n\
FLANNEL_IPMASQ=true\n";

#[test]
fn parses_minimal_happy_path() {
    let env = parse_subnet_env(HAPPY).unwrap();
    assert_eq!(env.network.to_string(), "10.244.0.0/16");
    // FLANNEL_SUBNET in upstream preserves the host bits (the address of the
    // node's gateway inside the lease).
    assert_eq!(env.subnet.to_string(), "10.244.7.1/24");
    assert_eq!(env.subnet.network().to_string(), "10.244.7.0");
    assert_eq!(env.mtu, 1450);
    assert!(env.ip_masq);
}

#[test]
fn ipmasq_false_path() {
    let s = HAPPY.replace("IPMASQ=true", "IPMASQ=false");
    let env = parse_subnet_env(&s).unwrap();
    assert!(!env.ip_masq);
}

#[test]
fn missing_network_is_error() {
    let s = "FLANNEL_SUBNET=10.244.1.0/24\nFLANNEL_MTU=1450\n";
    assert!(parse_subnet_env(s).is_err());
}

#[test]
fn missing_subnet_is_error() {
    let s = "FLANNEL_NETWORK=10.244.0.0/16\nFLANNEL_MTU=1450\n";
    assert!(parse_subnet_env(s).is_err());
}

#[test]
fn missing_mtu_is_error() {
    let s = "FLANNEL_NETWORK=10.244.0.0/16\nFLANNEL_SUBNET=10.244.1.0/24\n";
    assert!(parse_subnet_env(s).is_err());
}

#[test]
fn comments_and_blank_lines_skipped() {
    let s = format!("# comment line\n\n{HAPPY}\n# trailing\n");
    let env = parse_subnet_env(&s).unwrap();
    assert_eq!(env.mtu, 1450);
}

#[test]
fn unknown_keys_ignored() {
    let s = format!("{HAPPY}FLANNEL_SOMETHING_ELSE=ignored\n");
    let env = parse_subnet_env(&s).unwrap();
    assert_eq!(env.mtu, 1450);
}

#[test]
fn malformed_line_errors() {
    let s = "FLANNEL_NETWORK 10.244.0.0/16\n";
    assert!(parse_subnet_env(s).is_err());
}
