// SPDX-License-Identifier: Apache-2.0
//! Backend-agnostic proxy-rule IR + generation + incremental diff.
//!
//! kube-proxy's job is to turn (Services + EndpointSlices) into packet-
//! forwarding rules, then keep the kernel in sync by applying only the *delta*
//! on each change. We model that backend-independently:
//!
//! * [`ProxyRule`] — one frontend (VIP / NodePort / LB ingress + port) and the
//!   set of backend targets it load-balances over, plus the selected
//!   [`LoadBalanceMode`] and a [`RuleAction`] (forward vs. reject).
//! * [`build_rules`] — the pure generator: Services + slices in, deterministic
//!   `Vec<ProxyRule>` out.
//! * [`diff_rules`] — old vs. new rule sets → adds / removes, the incremental
//!   sync primitive.
//!
//! "Zero ready endpoints" is modelled explicitly: a programmed Service with no
//! usable backend yields a [`RuleAction::Reject`] frontend (so the packet is
//! rejected with ICMP rather than silently black-holed against a stale DNAT) —
//! this matches kube-proxy's KUBE-SERVICES reject rules for endpoint-less
//! Services.

use std::net::IpAddr;

use crate::core::model::{Protocol, Service, ServicePort, ServiceType};
use crate::core::select::{select_endpoints, EndpointTarget, NodeContext, SelectedEndpoints};
use crate::core::model::EndpointSlice;

/// Which frontend a rule fronts.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Frontend {
    /// Cluster VIP (`clusterIP:port`).
    ClusterIp { ip: IpAddr, port: u16 },
    /// Per-node port (`0.0.0.0:nodePort`).
    NodePort { port: u16 },
    /// LoadBalancer ingress IP (`ingressIP:port`).
    LoadBalancer { ip: IpAddr, port: u16 },
}

/// Load-balancing mode chosen for a rule's backends.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LoadBalanceMode {
    /// Stateless per-connection random/round-robin spread.
    Random,
    /// Client-IP affinity with a sticky TTL (seconds).
    ClientIpSticky { timeout_seconds: u32 },
}

/// What the proxy should do with packets hitting a frontend.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RuleAction {
    /// Forward to the listed backends using `mode`.
    Forward {
        mode: LoadBalanceMode,
        backends: Vec<EndpointTarget>,
    },
    /// Reject with ICMP (no usable backend) — explicit, not a black hole.
    Reject,
}

/// One computed forwarding rule.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProxyRule {
    pub service_key: String,
    /// Service port name (empty for single-port Services).
    pub port_name: String,
    pub protocol: Protocol,
    pub frontend: Frontend,
    pub action: RuleAction,
}

impl ProxyRule {
    /// Stable identity of the *frontend* this rule programs, independent of
    /// its backends/action. Two rules with the same key target the same
    /// kernel object; the diff keys on this.
    #[must_use]
    pub fn frontend_key(&self) -> String {
        let fe = match &self.frontend {
            Frontend::ClusterIp { ip, port } => format!("clusterip:{ip}:{port}"),
            Frontend::NodePort { port } => format!("nodeport:{port}"),
            Frontend::LoadBalancer { ip, port } => format!("lb:{ip}:{port}"),
        };
        format!("{}|{}|{}|{}", self.service_key, self.port_name, self.protocol.as_str(), fe)
    }
}

const fn lb_mode(svc: &Service) -> LoadBalanceMode {
    match svc.session_affinity {
        crate::core::model::SessionAffinity::None => LoadBalanceMode::Random,
        crate::core::model::SessionAffinity::ClientIp { timeout_seconds } => {
            LoadBalanceMode::ClientIpSticky { timeout_seconds }
        }
    }
}

fn action_for(svc: &Service, backends: Vec<EndpointTarget>) -> RuleAction {
    if backends.is_empty() {
        RuleAction::Reject
    } else {
        RuleAction::Forward {
            mode: lb_mode(svc),
            backends,
        }
    }
}

fn push_port_rules(
    out: &mut Vec<ProxyRule>,
    svc: &Service,
    sp: &ServicePort,
    sel: &SelectedEndpoints,
) {
    let key = svc.key();

    // ClusterIP frontend (every non-skipped Service has one).
    if let Some(ip) = svc.cluster_ip {
        out.push(ProxyRule {
            service_key: key.clone(),
            port_name: sp.name.clone(),
            protocol: sp.protocol,
            frontend: Frontend::ClusterIp { ip, port: sp.port },
            action: action_for(svc, sel.cluster.clone()),
        });
    }

    // NodePort frontend (NodePort + LoadBalancer types). External traffic, so
    // it uses the external-policy-filtered backend set.
    if matches!(svc.service_type, ServiceType::NodePort | ServiceType::LoadBalancer) {
        if let Some(np) = sp.node_port {
            out.push(ProxyRule {
                service_key: key.clone(),
                port_name: sp.name.clone(),
                protocol: sp.protocol,
                frontend: Frontend::NodePort { port: np },
                action: action_for(svc, sel.external.clone()),
            });
        }
    }

    // LoadBalancer ingress frontends.
    if matches!(svc.service_type, ServiceType::LoadBalancer) {
        for &lb in &svc.load_balancer_ips {
            out.push(ProxyRule {
                service_key: key.clone(),
                port_name: sp.name.clone(),
                protocol: sp.protocol,
                frontend: Frontend::LoadBalancer { ip: lb, port: sp.port },
                action: action_for(svc, sel.external.clone()),
            });
        }
    }
}

/// A Service plus all the slices backing it. Callers group inputs this way.
#[derive(Debug, Clone)]
pub struct ServiceInput<'a> {
    pub service: &'a Service,
    pub slices: &'a [EndpointSlice],
}

/// Generate the full deterministic set of [`ProxyRule`]s for the given
/// Services. Skipped Services (headless / ExternalName) produce no rules.
/// Output is sorted by [`ProxyRule::frontend_key`] for stable diffing.
#[must_use]
pub fn build_rules(inputs: &[ServiceInput<'_>], node: &NodeContext) -> Vec<ProxyRule> {
    let mut out = Vec::new();
    for input in inputs {
        let svc = input.service;
        if svc.should_skip() {
            continue;
        }
        for sp in &svc.ports {
            let sel = select_endpoints(svc, sp, input.slices, node);
            push_port_rules(&mut out, svc, sp, &sel);
        }
    }
    out.sort_by_key(ProxyRule::frontend_key);
    out
}

/// Result of comparing two rule sets: rules to program (`added`), rules to
/// tear down (`removed`), and rules whose frontend persists but whose
/// backends/action changed (`changed`, carrying the new rule).
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct RuleDiff {
    pub added: Vec<ProxyRule>,
    pub removed: Vec<ProxyRule>,
    pub changed: Vec<ProxyRule>,
}

impl RuleDiff {
    /// `true` when nothing needs to be applied to the kernel.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Compute the incremental sync delta between `old` and `new` rule sets.
///
/// Keyed on [`ProxyRule::frontend_key`]:
/// * present only in `new`  → `added`,
/// * present only in `old`  → `removed`,
/// * present in both but unequal → `changed` (the new value).
#[must_use]
pub fn diff_rules(old: &[ProxyRule], new: &[ProxyRule]) -> RuleDiff {
    use std::collections::BTreeMap;
    let old_map: BTreeMap<String, &ProxyRule> =
        old.iter().map(|r| (r.frontend_key(), r)).collect();
    let new_map: BTreeMap<String, &ProxyRule> =
        new.iter().map(|r| (r.frontend_key(), r)).collect();

    let mut diff = RuleDiff::default();
    for (k, nr) in &new_map {
        match old_map.get(k) {
            None => diff.added.push((*nr).clone()),
            Some(or) if or != nr => diff.changed.push((*nr).clone()),
            Some(_) => {}
        }
    }
    for (k, or) in &old_map {
        if !new_map.contains_key(k) {
            diff.removed.push((*or).clone());
        }
    }
    diff.added.sort_by_key(ProxyRule::frontend_key);
    diff.removed.sort_by_key(ProxyRule::frontend_key);
    diff.changed.sort_by_key(ProxyRule::frontend_key);
    diff
}
