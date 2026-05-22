// SPDX-License-Identifier: Apache-2.0
//! Internal data structures consumed by the rules builder. These mirror
//! upstream `pkg/proxy/iptables/proxier.go` `servicePortInfo` /
//! `endpointInfo` structs but trimmed to Phase 1 ClusterIP scope.

use crate::api::{Protocol, ServicePortName};

/// Upstream: `pkg/proxy/iptables/proxier.go servicePortInfo` (subset).
/// Carries everything `syncProxyRules` needs about one Service:Port.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServicePortInfo {
    pub name: ServicePortName,
    pub cluster_ip: String,
    pub port: i32,
    pub protocol: Protocol,
}

/// Upstream: `pkg/proxy/iptables/proxier.go endpointInfo` (subset).
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct EndpointInfo {
    pub ip: String,
    pub port: i32,
}

impl EndpointInfo {
    /// Upstream `BaseEndpointInfo.String()` — used as the third hash input
    /// to `servicePortEndpointChainName` and as the DNAT target.
    #[must_use]
    pub fn endpoint_string(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }
}

/// Upstream iptables tables we touch (`pkg/util/iptables` `Table` const).
/// Phase 1 only writes `nat`; `filter` is empty-init for parity but no
/// rules are emitted there yet (NodePort/LB rules live in `filter`).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Table {
    Nat,
    Filter,
}

impl Table {
    #[must_use]
    pub const fn header(self) -> &'static str {
        match self {
            Self::Nat => "*nat",
            Self::Filter => "*filter",
        }
    }
}

/// One emitted iptables-restore line, classified by table for grouping.
/// The Display impl renders the raw text — `IptablesRule::to_string()`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct IptablesRule {
    pub table: Table,
    pub text: String,
}

impl std::fmt::Display for IptablesRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
