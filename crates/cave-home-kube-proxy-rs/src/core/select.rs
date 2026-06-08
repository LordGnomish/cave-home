// SPDX-License-Identifier: Apache-2.0
//! Endpoint selection — the heart of the proxy decision core.
//!
//! Given a Service's backing [`EndpointSlice`]s plus the local node's identity
//! and zone, decide which endpoints actually receive traffic, separately for
//! the two traffic classes kube-proxy distinguishes:
//!
//! * **cluster traffic** — packets that already hit the cluster VIP. Spread
//!   over every ready+serving, non-terminating endpoint.
//! * **external traffic** — packets that entered via a NodePort / LoadBalancer.
//!   When `externalTrafficPolicy: Local`, only endpoints *on this node* are
//!   eligible (preserving client source IP and avoiding a second hop);
//!   otherwise the cluster set is reused.
//!
//! Two refinements layer on top, matching documented kube-proxy behaviour:
//!
//! * **topology-aware hints** — when every ready endpoint publishes a
//!   `hints.forZones`, restrict the cluster set to endpoints hinted for this
//!   node's zone. If applying the hint would empty the set, the hint is
//!   ignored (fail-open), exactly as upstream does.
//! * **terminating fallback** — if the ready set is empty but
//!   serving-terminating endpoints exist, use those rather than black-holing.

use std::collections::BTreeSet;
use std::net::IpAddr;

use crate::core::model::{EndpointSlice, Protocol, Service, ServicePort};

/// One concrete backend target a connection can be sent to.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct EndpointTarget {
    pub ip: IpAddr,
    pub port: u16,
    /// `true` iff this endpoint lives on the local node (drives `Local` policy).
    pub local: bool,
    pub zone: Option<String>,
}

/// The local node's identity, needed for `Local` policy + topology hints.
#[derive(Debug, Clone, Default)]
pub struct NodeContext {
    pub node_name: Option<String>,
    pub zone: Option<String>,
}

/// Resolve the [`EndpointPort`](crate::core::model::EndpointPort) number that
/// backs a given Service port, by matching port *names*. Single-port Services
/// (empty name on both sides) match unconditionally; otherwise the slice port
/// whose name equals the Service port name wins. Falls back to the numeric
/// `target_port`.
fn resolve_target_port(svc_port: &ServicePort, slice: &EndpointSlice) -> Option<u16> {
    for ep in &slice.ports {
        if ep.protocol == svc_port.protocol && ep.name == svc_port.name {
            return Some(ep.port);
        }
    }
    // No matching named port on this slice; fall back to the numeric target.
    if svc_port.target_port != 0 {
        Some(svc_port.target_port)
    } else {
        None
    }
}

/// Collect every endpoint backing `svc_port` from all `slices`, with its
/// resolved target port, computed `local` flag, and readiness classification.
struct Classified {
    target: EndpointTarget,
    ready_serving: bool,
    serving_terminating: bool,
}

fn classify_all(
    svc_port: &ServicePort,
    slices: &[EndpointSlice],
    node: &NodeContext,
) -> Vec<Classified> {
    let mut out = Vec::new();
    for slice in slices {
        let Some(target_port) = resolve_target_port(svc_port, slice) else {
            continue;
        };
        for ep in &slice.endpoints {
            let local = match (&node.node_name, &ep.node_name) {
                (Some(n), Some(e)) => n == e,
                _ => false,
            };
            for &ip in &ep.addresses {
                out.push(Classified {
                    target: EndpointTarget {
                        ip,
                        port: target_port,
                        local,
                        zone: ep.zone.clone(),
                    },
                    ready_serving: ep.is_ready_serving(),
                    serving_terminating: ep.is_serving_terminating(),
                });
            }
        }
    }
    out
}

/// Apply topology-aware hints to a ready set. Returns the hint-filtered subset
/// when *all* candidates carry hints and the result is non-empty; otherwise
/// returns the input unchanged (fail-open), matching upstream
/// `pkg/proxy/topology.go`.
fn apply_topology_hints(eps: Vec<(EndpointTarget, Vec<String>)>, node: &NodeContext) -> Vec<EndpointTarget> {
    let Some(my_zone) = node.zone.as_deref() else {
        return eps.into_iter().map(|(t, _)| t).collect();
    };
    // Hints only apply if every endpoint published at least one zone hint.
    let all_hinted = !eps.is_empty() && eps.iter().all(|(_, h)| !h.is_empty());
    if !all_hinted {
        return eps.into_iter().map(|(t, _)| t).collect();
    }
    let filtered: Vec<EndpointTarget> = eps
        .iter()
        .filter(|(_, h)| h.iter().any(|z| z == my_zone))
        .map(|(t, _)| t.clone())
        .collect();
    if filtered.is_empty() {
        // Hint would black-hole this zone — ignore it.
        eps.into_iter().map(|(t, _)| t).collect()
    } else {
        filtered
    }
}

/// Selected endpoints for a Service port, split by traffic class.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct SelectedEndpoints {
    /// Targets for in-cluster (VIP) traffic.
    pub cluster: Vec<EndpointTarget>,
    /// Targets for external (NodePort / LoadBalancer) traffic. Under
    /// `Local` policy these are the local-node subset; under `Cluster` they
    /// equal `cluster`.
    pub external: Vec<EndpointTarget>,
    /// `true` when these targets are terminating-only (last-resort fallback).
    pub from_terminating: bool,
}

fn dedup_sorted(mut v: Vec<EndpointTarget>) -> Vec<EndpointTarget> {
    v.sort();
    v.dedup();
    v
}

/// Run endpoint selection for one Service port.
///
/// Steps:
/// 1. classify every backing endpoint (ready/serving/terminating, local-ness),
/// 2. take the ready+serving non-terminating set; if empty, fall back to the
///    serving-terminating set,
/// 3. apply topology hints to the cluster set,
/// 4. derive the external set from `externalTrafficPolicy`.
#[must_use]
pub fn select_endpoints(
    svc: &Service,
    svc_port: &ServicePort,
    slices: &[EndpointSlice],
    node: &NodeContext,
) -> SelectedEndpoints {
    use crate::core::model::ExternalTrafficPolicy;

    let classified = classify_all(svc_port, slices, node);

    let ready: Vec<&Classified> = classified.iter().filter(|c| c.ready_serving).collect();
    let (working, from_terminating): (Vec<&Classified>, bool) = if ready.is_empty() {
        (
            classified.iter().filter(|c| c.serving_terminating).collect(),
            true,
        )
    } else {
        (ready, false)
    };

    // Cluster set, with topology hints applied. We must re-pair each target
    // with its endpoint's hints; rebuild that from the classified inputs.
    let hinted: Vec<(EndpointTarget, Vec<String>)> = working
        .iter()
        .map(|c| {
            let hints = hints_for(svc_port, slices, &c.target.ip);
            (c.target.clone(), hints)
        })
        .collect();
    let cluster = dedup_sorted(apply_topology_hints(hinted, node));

    // External set.
    let external = match svc.external_traffic_policy {
        ExternalTrafficPolicy::Cluster => cluster.clone(),
        ExternalTrafficPolicy::Local => {
            dedup_sorted(working.iter().filter(|c| c.target.local).map(|c| c.target.clone()).collect())
        }
    };

    SelectedEndpoints {
        cluster,
        external,
        from_terminating,
    }
}

/// Look up the `hints.forZones` published for the endpoint owning `ip`.
fn hints_for(svc_port: &ServicePort, slices: &[EndpointSlice], ip: &IpAddr) -> Vec<String> {
    for slice in slices {
        // Only consider slices that actually back this Service port.
        if resolve_target_port(svc_port, slice).is_none() {
            continue;
        }
        for ep in &slice.endpoints {
            if ep.addresses.contains(ip) {
                return ep.hints_for_zones.clone();
            }
        }
    }
    Vec::new()
}

/// Convenience helper exposing only ready+serving, non-terminating endpoints
/// (ignoring topology + policy) — used by callers/tests that want the raw
/// reachable set.
#[must_use]
pub fn ready_targets(
    svc_port: &ServicePort,
    slices: &[EndpointSlice],
    node: &NodeContext,
) -> Vec<EndpointTarget> {
    let classified = classify_all(svc_port, slices, node);
    dedup_sorted(
        classified
            .into_iter()
            .filter(|c| c.ready_serving)
            .map(|c| c.target)
            .collect(),
    )
}

/// All distinct zones present across the ready endpoints of a Service port.
#[must_use]
pub fn zones_present(svc_port: &ServicePort, slices: &[EndpointSlice]) -> BTreeSet<String> {
    let node = NodeContext::default();
    classify_all(svc_port, slices, &node)
        .into_iter()
        .filter(|c| c.ready_serving)
        .filter_map(|c| c.target.zone)
        .collect()
}

/// The wire protocol for a Service port (re-exported for rule builders).
#[must_use]
pub const fn port_protocol(p: &ServicePort) -> Protocol {
    p.protocol
}
