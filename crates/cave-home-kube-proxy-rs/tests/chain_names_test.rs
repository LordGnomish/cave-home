// SPDX-License-Identifier: Apache-2.0
//! Verifies the `portProtoHash` / `servicePortEndpointChainName` algorithm
//! bit-for-bit against upstream `pkg/proxy/iptables/proxier.go` fixtures
//! (collected from `proxier_test.go` `KUBE-(SVC|SEP|EXT|FW|SVL)-XXXX` chain
//! literals — those literals ARE the test oracle).
//!
//! Algorithm (upstream lines 546-553):
//!     hash = sha256(servicePortName + protocol)
//!     encoded = base32.StdEncoding.EncodeToString(hash[:])
//!     return encoded[:16]
//! Endpoint variant (upstream lines 590-594):
//!     hash = sha256(servicePortName + protocol + endpoint)
//!     encoded = base32.StdEncoding.EncodeToString(hash[:])
//!     return encoded[:16]

use cave_home_kube_proxy_rs::iptables::chain_names::{
    port_proto_hash, service_external_chain_name, service_firewall_chain_name,
    service_port_endpoint_chain_name, service_port_policy_cluster_chain,
    service_port_policy_local_chain_name,
};

// --- portProtoHash fixtures from upstream proxier_test.go ---------------------

#[test]
fn port_proto_hash_ns1_svc1_p80_tcp() {
    // Upstream KUBE-SVC-XPGD46QRK7WJZT7O — appears 100+ times in proxier_test.go
    assert_eq!(port_proto_hash("ns1/svc1:p80", "tcp"), "XPGD46QRK7WJZT7O");
}

#[test]
fn port_proto_hash_ns2_svc2_p80_tcp() {
    // Upstream KUBE-SVC-GNZBNJ2PO5MGZ6GT
    assert_eq!(port_proto_hash("ns2/svc2:p80", "tcp"), "GNZBNJ2PO5MGZ6GT");
}

#[test]
fn port_proto_hash_ns5_svc5_p80_tcp() {
    // Upstream KUBE-FW-NUKIZ6OKUXPJNT4C (KUBE-SVC- shares same hash)
    assert_eq!(port_proto_hash("ns5/svc5:p80", "tcp"), "NUKIZ6OKUXPJNT4C");
}

// --- service_port_policy_cluster_chain (KUBE-SVC-) ---------------------------

#[test]
fn service_port_policy_cluster_chain_prefixes_kube_svc() {
    assert_eq!(
        service_port_policy_cluster_chain("ns1/svc1:p80", "tcp"),
        "KUBE-SVC-XPGD46QRK7WJZT7O"
    );
    assert_eq!(
        service_port_policy_cluster_chain("ns2/svc2:p80", "tcp"),
        "KUBE-SVC-GNZBNJ2PO5MGZ6GT"
    );
}

// --- service_port_policy_local_chain_name (KUBE-SVL-) ------------------------

#[test]
fn service_port_policy_local_chain_name_prefixes_kube_svl() {
    // Upstream KUBE-SVL-GNZBNJ2PO5MGZ6GT exists in proxier_test.go fixtures.
    assert_eq!(
        service_port_policy_local_chain_name("ns2/svc2:p80", "tcp"),
        "KUBE-SVL-GNZBNJ2PO5MGZ6GT"
    );
}

// --- service_firewall_chain_name (KUBE-FW-) ----------------------------------

#[test]
fn service_firewall_chain_name_prefixes_kube_fw() {
    // Upstream KUBE-FW-NUKIZ6OKUXPJNT4C
    assert_eq!(
        service_firewall_chain_name("ns5/svc5:p80", "tcp"),
        "KUBE-FW-NUKIZ6OKUXPJNT4C"
    );
}

// --- service_external_chain_name (KUBE-EXT-) ---------------------------------

#[test]
fn service_external_chain_name_prefixes_kube_ext() {
    // Upstream KUBE-EXT-GNZBNJ2PO5MGZ6GT
    assert_eq!(
        service_external_chain_name("ns2/svc2:p80", "tcp"),
        "KUBE-EXT-GNZBNJ2PO5MGZ6GT"
    );
}

// --- service_port_endpoint_chain_name (KUBE-SEP-) ----------------------------

#[test]
fn service_port_endpoint_chain_name_for_ns1_svc1_endpoint() {
    // Upstream proxier_test.go fixture (line 212):
    //   KUBE-SEP-SXIVWICOYRO3J4NJ for endpoint 10.180.0.1:80
    assert_eq!(
        service_port_endpoint_chain_name("ns1/svc1:p80", "tcp", "10.180.0.1:80"),
        "KUBE-SEP-SXIVWICOYRO3J4NJ"
    );
}

#[test]
fn chain_names_are_28_chars_or_less() {
    // Upstream: iptables chain names must be <= 28 chars.
    let svc = service_port_policy_cluster_chain("very-long-namespace/very-long-service:portname", "tcp");
    assert!(svc.len() <= 28, "got {} ({} chars)", svc, svc.len());
    let sep = service_port_endpoint_chain_name(
        "very-long-namespace/very-long-service:portname",
        "tcp",
        "192.168.255.255:65535",
    );
    assert!(sep.len() <= 28, "got {} ({} chars)", sep, sep.len());
}

#[test]
fn port_proto_hash_is_pure_no_state() {
    // Calling twice yields identical output.
    assert_eq!(
        port_proto_hash("ns1/svc1:p80", "tcp"),
        port_proto_hash("ns1/svc1:p80", "tcp")
    );
}

#[test]
fn port_proto_hash_distinguishes_protocol() {
    // Different protocol must yield different hash.
    let tcp = port_proto_hash("ns1/svc1:p80", "tcp");
    let udp = port_proto_hash("ns1/svc1:p80", "udp");
    assert_ne!(tcp, udp);
}
