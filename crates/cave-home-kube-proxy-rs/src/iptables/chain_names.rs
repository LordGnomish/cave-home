// SPDX-License-Identifier: Apache-2.0
//! Chain-naming helpers — a behavioural reimplementation of the *documented*
//! kube-proxy iptables chain-naming scheme (`KUBE-SVC-`/`KUBE-SEP-`/… +
//! truncated base32(SHA-256) hash). The expected chain literals used in
//! `tests/chain_names_test.rs` are the well-known published values for the
//! canonical `ns1/svc1:p80` fixtures; this is verified against those documented
//! values, not against any specific unread upstream source revision.
//!
//! Algorithm (as publicly documented, Go):
//! ```go
//! func portProtoHash(servicePortName string, protocol string) string {
//!     hash := sha256.Sum256([]byte(servicePortName + protocol))
//!     encoded := base32.StdEncoding.EncodeToString(hash[:])
//!     return encoded[:16]
//! }
//! ```
//! `base32.StdEncoding` in Go is RFC 4648 standard alphabet WITH `=` padding —
//! we slice `[:16]` so padding never appears in the output, but we must use
//! the standard (uppercase A-Z 2-7) alphabet, NOT the hex variant.

use sha2::{Digest, Sha256};

/// Upstream prefix constants (`pkg/proxy/iptables/proxier.go` lines 553-558).
pub const SERVICE_PORT_POLICY_CLUSTER_CHAIN_NAME_PREFIX: &str = "KUBE-SVC-";
pub const SERVICE_PORT_POLICY_LOCAL_CHAIN_NAME_PREFIX: &str = "KUBE-SVL-";
pub const SERVICE_FIREWALL_CHAIN_NAME_PREFIX: &str = "KUBE-FW-";
pub const SERVICE_EXTERNAL_CHAIN_NAME_PREFIX: &str = "KUBE-EXT-";
pub const SERVICE_PORT_ENDPOINT_CHAIN_NAME_PREFIX: &str = "KUBE-SEP-";

/// Upstream `portProtoHash` — SHA-256 → RFC4648 base32 → first 16 chars.
#[must_use]
pub fn port_proto_hash(service_port_name: &str, protocol: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(service_port_name.as_bytes());
    hasher.update(protocol.as_bytes());
    let digest = hasher.finalize();
    // base32::Alphabet::Rfc4648 with padding=true is Go's base32.StdEncoding.
    // Padding never reaches the slice since the SHA-256 digest is 32 bytes,
    // which encodes to 56 base32 characters — well over the 16 we keep.
    let encoded = base32::encode(base32::Alphabet::Rfc4648 { padding: true }, &digest);
    encoded.chars().take(16).collect()
}

/// Upstream `servicePortPolicyClusterChain` — main `KUBE-SVC-XXXX` chain.
#[must_use]
pub fn service_port_policy_cluster_chain(service_port_name: &str, protocol: &str) -> String {
    format!(
        "{}{}",
        SERVICE_PORT_POLICY_CLUSTER_CHAIN_NAME_PREFIX,
        port_proto_hash(service_port_name, protocol)
    )
}

/// Upstream `servicePortPolicyLocalChainName` — `KUBE-SVL-XXXX` (Local policy).
#[must_use]
pub fn service_port_policy_local_chain_name(service_port_name: &str, protocol: &str) -> String {
    format!(
        "{}{}",
        SERVICE_PORT_POLICY_LOCAL_CHAIN_NAME_PREFIX,
        port_proto_hash(service_port_name, protocol)
    )
}

/// Upstream `serviceFirewallChainName` — `KUBE-FW-XXXX` (LB source ranges).
#[must_use]
pub fn service_firewall_chain_name(service_port_name: &str, protocol: &str) -> String {
    format!(
        "{}{}",
        SERVICE_FIREWALL_CHAIN_NAME_PREFIX,
        port_proto_hash(service_port_name, protocol)
    )
}

/// Upstream `serviceExternalChainName` — `KUBE-EXT-XXXX` (external traffic).
#[must_use]
pub fn service_external_chain_name(service_port_name: &str, protocol: &str) -> String {
    format!(
        "{}{}",
        SERVICE_EXTERNAL_CHAIN_NAME_PREFIX,
        port_proto_hash(service_port_name, protocol)
    )
}

/// Upstream `servicePortEndpointChainName` — `KUBE-SEP-XXXX` per endpoint.
#[must_use]
pub fn service_port_endpoint_chain_name(
    service_port_name: &str,
    protocol: &str,
    endpoint: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(service_port_name.as_bytes());
    hasher.update(protocol.as_bytes());
    hasher.update(endpoint.as_bytes());
    let digest = hasher.finalize();
    let encoded = base32::encode(base32::Alphabet::Rfc4648 { padding: true }, &digest);
    let truncated: String = encoded.chars().take(16).collect();
    format!("{SERVICE_PORT_ENDPOINT_CHAIN_NAME_PREFIX}{truncated}")
}
