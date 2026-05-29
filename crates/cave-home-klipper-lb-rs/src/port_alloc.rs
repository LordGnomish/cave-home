// SPDX-License-Identifier: Apache-2.0
//! Host-port allocation + conflict detection across LoadBalancer Services.
//!
//! klipper-lb's svclb pod binds the Service's published `port` on the *host*
//! network of every node it runs on (`hostPort`), then forwards to the cluster
//! Service's NodePort. Because every svclb pod for a given Service shares the
//! same host port on every node, two LoadBalancer Services that want the same
//! `(protocol, port)` cannot both be bound — the second one conflicts and is
//! skipped, exactly as K3s leaves the second Service `<pending>`.
//!
//! This module is the bookkeeping for that: it tracks which `(protocol, port)`
//! pairs are claimed and by which Service, and reports conflicts instead of
//! silently double-binding.

use std::collections::BTreeMap;

use crate::service::{LoadBalancerService, Protocol};

/// A host-port claim: a `(protocol, port)` pair reserved by a Service.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct HostPort {
    pub protocol: Protocol,
    pub port: u16,
}

impl HostPort {
    #[must_use]
    pub const fn new(protocol: Protocol, port: u16) -> Self {
        Self { protocol, port }
    }
}

/// A host-port conflict: `port` was already claimed by `held_by` when
/// `requested_by` asked for it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PortConflict {
    pub port: HostPort,
    /// Service key (`namespace/name`) that already holds the port.
    pub held_by: String,
    /// Service key that was rejected because of the conflict.
    pub requested_by: String,
}

/// Outcome of trying to allocate one Service's host ports.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AllocationOutcome {
    /// All of the Service's host ports were free and are now claimed.
    Allocated { ports: Vec<HostPort> },
    /// At least one host port was already held; nothing was claimed for this
    /// Service (svclb is all-or-nothing per Service — a partial bind would
    /// leave the Service half-exposed).
    Conflicted { conflicts: Vec<PortConflict> },
}

impl AllocationOutcome {
    /// True iff the Service was successfully allocated.
    #[must_use]
    pub const fn is_allocated(&self) -> bool {
        matches!(self, Self::Allocated { .. })
    }
}

/// Tracks host-port claims across every admitted LoadBalancer Service on a
/// node's host network.
#[derive(Debug, Clone, Default)]
pub struct HostPortAllocator {
    /// `(protocol, port)` -> owning Service key.
    claims: BTreeMap<HostPort, String>,
}

impl HostPortAllocator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The host ports a Service wants, derived from its `port` entries.
    fn requested(svc: &LoadBalancerService) -> Vec<HostPort> {
        svc.ports
            .iter()
            .map(|p| HostPort::new(p.protocol, p.port))
            .collect()
    }

    /// Attempt to claim every host port the Service needs.
    ///
    /// All-or-nothing: if any port is already held by a *different* Service the
    /// whole Service is reported as [`AllocationOutcome::Conflicted`] and no new
    /// claim is recorded. Re-allocating a Service that already owns its ports is
    /// idempotent (the claims simply stay).
    pub fn allocate(&mut self, svc: &LoadBalancerService) -> AllocationOutcome {
        let key = svc.key();
        let wanted = Self::requested(svc);

        let mut conflicts = Vec::new();
        for hp in &wanted {
            if let Some(holder) = self.claims.get(hp) {
                if holder != &key {
                    conflicts.push(PortConflict {
                        port: *hp,
                        held_by: holder.clone(),
                        requested_by: key.clone(),
                    });
                }
            }
        }
        if !conflicts.is_empty() {
            return AllocationOutcome::Conflicted { conflicts };
        }

        for hp in &wanted {
            self.claims.insert(*hp, key.clone());
        }
        AllocationOutcome::Allocated { ports: wanted }
    }

    /// Release every host port held by the given Service key (Service deleted /
    /// type changed away from LoadBalancer). Returns the freed ports.
    pub fn release(&mut self, key: &str) -> Vec<HostPort> {
        let freed: Vec<HostPort> = self
            .claims
            .iter()
            .filter(|(_, owner)| owner.as_str() == key)
            .map(|(hp, _)| *hp)
            .collect();
        for hp in &freed {
            self.claims.remove(hp);
        }
        freed
    }

    /// The Service key currently holding `port`, if any.
    #[must_use]
    pub fn holder(&self, port: HostPort) -> Option<&str> {
        self.claims.get(&port).map(String::as_str)
    }

    /// Number of host ports currently claimed.
    #[must_use]
    pub fn claimed_count(&self) -> usize {
        self.claims.len()
    }
}

/// Allocate a batch of Services in the given (admission) order, returning each
/// Service key paired with its outcome. Earlier Services win ports; later
/// conflicting Services are skipped — matching K3s's first-come ordering.
#[must_use]
pub fn allocate_all(
    svcs: &[LoadBalancerService],
) -> Vec<(String, AllocationOutcome)> {
    let mut alloc = HostPortAllocator::new();
    svcs.iter()
        .map(|s| (s.key(), alloc.allocate(s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::{ExternalTrafficPolicy, ServicePort};

    fn svc(ns: &str, name: &str, ports: &[(Protocol, u16, u16)]) -> LoadBalancerService {
        LoadBalancerService {
            namespace: ns.to_owned(),
            name: name.to_owned(),
            load_balancer_ips: vec![],
            ports: ports
                .iter()
                .enumerate()
                .map(|(i, (proto, port, np))| ServicePort {
                    name: format!("p{i}"),
                    protocol: *proto,
                    port: *port,
                    node_port: *np,
                })
                .collect(),
            external_traffic_policy: ExternalTrafficPolicy::Cluster,
            node_selector: BTreeMap::new(),
        }
    }

    #[test]
    fn first_service_allocates_cleanly() {
        let mut a = HostPortAllocator::new();
        let out = a.allocate(&svc("default", "web", &[(Protocol::Tcp, 80, 30080)]));
        assert!(out.is_allocated());
        assert_eq!(a.claimed_count(), 1);
        assert_eq!(a.holder(HostPort::new(Protocol::Tcp, 80)), Some("default/web"));
    }

    #[test]
    fn second_service_same_port_conflicts() {
        let mut a = HostPortAllocator::new();
        assert!(a
            .allocate(&svc("default", "web", &[(Protocol::Tcp, 80, 30080)]))
            .is_allocated());
        let out = a.allocate(&svc("default", "blog", &[(Protocol::Tcp, 80, 30081)]));
        match out {
            AllocationOutcome::Conflicted { conflicts } => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].held_by, "default/web");
                assert_eq!(conflicts[0].requested_by, "default/blog");
                assert_eq!(conflicts[0].port, HostPort::new(Protocol::Tcp, 80));
            }
            AllocationOutcome::Allocated { .. } => panic!("expected conflict"),
        }
        // The conflicting Service claimed nothing.
        assert_eq!(a.claimed_count(), 1);
    }

    #[test]
    fn same_port_different_protocol_does_not_conflict() {
        let mut a = HostPortAllocator::new();
        assert!(a
            .allocate(&svc("default", "dns-t", &[(Protocol::Tcp, 53, 30053)]))
            .is_allocated());
        assert!(a
            .allocate(&svc("default", "dns-u", &[(Protocol::Udp, 53, 30054)]))
            .is_allocated());
        assert_eq!(a.claimed_count(), 2);
    }

    #[test]
    fn conflict_is_all_or_nothing() {
        // Service wants 80 (free) and 443 (held) -> nothing is claimed.
        let mut a = HostPortAllocator::new();
        assert!(a
            .allocate(&svc("default", "tls", &[(Protocol::Tcp, 443, 30443)]))
            .is_allocated());
        let out = a.allocate(&svc(
            "default",
            "web",
            &[(Protocol::Tcp, 80, 30080), (Protocol::Tcp, 443, 30444)],
        ));
        assert!(!out.is_allocated());
        // 80 must NOT have been claimed by the rejected service.
        assert_eq!(a.holder(HostPort::new(Protocol::Tcp, 80)), None);
        assert_eq!(a.holder(HostPort::new(Protocol::Tcp, 443)), Some("default/tls"));
    }

    #[test]
    fn reallocating_same_service_is_idempotent() {
        let mut a = HostPortAllocator::new();
        let s = svc("default", "web", &[(Protocol::Tcp, 80, 30080)]);
        assert!(a.allocate(&s).is_allocated());
        assert!(a.allocate(&s).is_allocated());
        assert_eq!(a.claimed_count(), 1);
    }

    #[test]
    fn release_frees_ports_for_reuse() {
        let mut a = HostPortAllocator::new();
        assert!(a
            .allocate(&svc("default", "web", &[(Protocol::Tcp, 80, 30080)]))
            .is_allocated());
        let freed = a.release("default/web");
        assert_eq!(freed, vec![HostPort::new(Protocol::Tcp, 80)]);
        assert_eq!(a.claimed_count(), 0);
        // Now a different service can take port 80.
        assert!(a
            .allocate(&svc("default", "blog", &[(Protocol::Tcp, 80, 30081)]))
            .is_allocated());
    }

    #[test]
    fn release_unknown_service_frees_nothing() {
        let mut a = HostPortAllocator::new();
        assert!(a.release("ghost/none").is_empty());
    }

    #[test]
    fn allocate_all_first_come_first_served() {
        let svcs = vec![
            svc("default", "web", &[(Protocol::Tcp, 80, 30080)]),
            svc("default", "blog", &[(Protocol::Tcp, 80, 30081)]),
            svc("default", "api", &[(Protocol::Tcp, 8080, 30082)]),
        ];
        let out = allocate_all(&svcs);
        assert_eq!(out[0].0, "default/web");
        assert!(out[0].1.is_allocated());
        assert_eq!(out[1].0, "default/blog");
        assert!(!out[1].1.is_allocated()); // conflict on 80
        assert_eq!(out[2].0, "default/api");
        assert!(out[2].1.is_allocated()); // 8080 is free
    }

    #[test]
    fn multi_port_service_claims_all_its_ports() {
        let mut a = HostPortAllocator::new();
        let out = a.allocate(&svc(
            "default",
            "web",
            &[(Protocol::Tcp, 80, 30080), (Protocol::Tcp, 443, 30443)],
        ));
        match out {
            AllocationOutcome::Allocated { ports } => assert_eq!(ports.len(), 2),
            AllocationOutcome::Conflicted { .. } => panic!("expected allocation"),
        }
        assert_eq!(a.claimed_count(), 2);
    }
}
