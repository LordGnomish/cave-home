// SPDX-License-Identifier: Apache-2.0
//! The ServiceLB controller **reconcile loop** decision core.
//!
//! K3s's ServiceLB controller (`pkg/cloudprovider/servicelb.go`) reacts to
//! Service / Node / Pod / EndpointSlice events and, for each affected Service,
//! decides whether to *deploy* the svclb `DaemonSet` (`deployDaemonSet`), *delete*
//! it (`deleteDaemonSet`), and what the Service's `status.loadBalancer.ingress`
//! should become (`updateStatus` → `getStatus` → `podIPs`).
//!
//! That controller is, at heart, a pure function from a **cluster snapshot** to a
//! set of intended actions. This module is exactly that pure function — it
//! composes the already-tested pieces of this crate:
//!
//! * [`crate::service::validate_service`] — drop structurally bad Services,
//! * [`crate::port_alloc::HostPortAllocator`] — first-come host-port admission;
//!   a second LB Service contending for a `(protocol, port)` is left pending,
//! * [`crate::daemonset::build_pod_spec`] — the svclb pod spec to apply,
//! * [`crate::status::compute_ingress_ips`] — the ingress IPs to patch back,
//!
//! into a single [`reconcile`] that returns a [`Reconciliation`]: the DaemonSets
//! to apply, the orphan DaemonSets to delete, and the per-Service
//! [`ServiceDisposition`].
//!
//! What stays out (ADR-004 Phase 1b, see `parity.manifest.toml`): the apiserver
//! client that turns [`Reconciliation::apply`] / [`Reconciliation::delete`] into
//! real `DaemonSet` create/update/delete calls and patches the Service status,
//! and the informer/watch that delivers the events driving each reconcile. This
//! function recomputes deterministically from a snapshot, so it is event-source
//! agnostic — the watch is pure glue on top.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use crate::daemonset::{build_pod_spec, SvclbPodSpec};
use crate::node::Node;
use crate::port_alloc::{AllocationOutcome, HostPortAllocator, PortConflict};
use crate::service::{validate_service, LoadBalancerService, ServiceError};
use crate::status::compute_ingress_ips;

/// The non-Service inputs a reconcile needs, all keyed by Service `key()`
/// (`namespace/name`). Every field is optional with a sensible empty default, so
/// the common case is `ReconcileContext::default()`.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ReconcileContext {
    /// Per-Service: names of nodes running a *ready backing pod*. Only consulted
    /// for `externalTrafficPolicy: Local` Services (see
    /// [`crate::status::compute_ingress_ips`]).
    pub backing_pods: BTreeMap<String, BTreeSet<String>>,
    /// Per-Service: the in-cluster destination IP(s) (the Service ClusterIP) the
    /// svclb containers forward to — emitted as the `DEST_IPS` env var. A Service
    /// with no entry forwards to an empty `DEST_IPS` (the caller resolves these).
    pub dest_ips: BTreeMap<String, Vec<IpAddr>>,
    /// Names (`svclb-<name>`) of svclb DaemonSets that currently exist in the
    /// cluster. Any not backing a programmed Service this reconcile is an orphan
    /// to delete.
    pub existing_daemonsets: BTreeSet<String>,
}

impl ReconcileContext {
    /// Builder: set the existing svclb DaemonSet names (for orphan detection).
    #[must_use]
    pub fn with_existing_daemonsets<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.existing_daemonsets = names.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: set the per-Service `DEST_IPS` map.
    #[must_use]
    pub fn with_dest_ips(mut self, dest_ips: BTreeMap<String, Vec<IpAddr>>) -> Self {
        self.dest_ips = dest_ips;
        self
    }

    /// Builder: set the per-Service ready-backing-pod node map (Local ETP).
    #[must_use]
    pub fn with_backing_pods(mut self, backing_pods: BTreeMap<String, BTreeSet<String>>) -> Self {
        self.backing_pods = backing_pods;
        self
    }
}

/// What the controller decided for one Service in a reconcile.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ServiceDisposition {
    /// Admitted: a svclb `DaemonSet` (named `daemonset`) should exist for this
    /// Service, and its `status.loadBalancer.ingress` should be `ingress_ips`.
    /// The list may be empty when no eligible node can yet advertise an IP (the
    /// DaemonSet is still deployed — status stays pending until a node is ready),
    /// matching K3s deploying the DaemonSet before any pod is ready.
    Programmed {
        daemonset: String,
        ingress_ips: Vec<IpAddr>,
    },
    /// Left pending: another LB Service already holds one of this Service's host
    /// `(protocol, port)` pairs. No DaemonSet is applied; status stays empty —
    /// exactly as K3s leaves the second contending Service `<pending>`.
    Pending { conflicts: Vec<PortConflict> },
    /// Rejected by structural validation; skipped (the controller logs and moves
    /// on) — it claims no host port and gets no DaemonSet.
    Invalid { reason: ServiceError },
}

impl ServiceDisposition {
    /// True iff the Service was admitted and a DaemonSet should be applied.
    #[must_use]
    pub const fn is_programmed(&self) -> bool {
        matches!(self, Self::Programmed { .. })
    }

    /// The ingress IPs this Service publishes — empty for anything not
    /// [`Programmed`](Self::Programmed).
    #[must_use]
    pub fn ingress_ips(&self) -> &[IpAddr] {
        match self {
            Self::Programmed { ingress_ips, .. } => ingress_ips,
            Self::Pending { .. } | Self::Invalid { .. } => &[],
        }
    }
}

/// The outcome of one reconcile pass over a cluster snapshot.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Reconciliation {
    /// Per-Service disposition, in the input (admission) order.
    pub dispositions: Vec<(String, ServiceDisposition)>,
    /// svclb pod specs to **create-or-update** (`deployDaemonSet` / `Apply`), in
    /// admission order — one per programmed Service.
    pub apply: Vec<SvclbPodSpec>,
    /// svclb `DaemonSet` names to **delete** (`deleteDaemonSet`): DaemonSets that
    /// exist but no longer back a programmed Service. Sorted, de-duplicated.
    pub delete: Vec<String>,
}

impl Reconciliation {
    /// Look up the disposition for a Service by its `namespace/name` key.
    #[must_use]
    pub fn disposition(&self, key: &str) -> Option<&ServiceDisposition> {
        self.dispositions
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, d)| d)
    }

    /// The set of svclb DaemonSet names this reconcile wants to exist.
    #[must_use]
    pub fn desired_daemonsets(&self) -> BTreeSet<String> {
        self.apply.iter().map(|s| s.daemonset_name.clone()).collect()
    }

    /// The `status.loadBalancer.ingress` patches to push back: one
    /// `(service_key, ingress_ips)` pair per programmed Service, in admission
    /// order. (Pending / invalid Services are not patched here — their status is
    /// left/cleared by the caller.)
    #[must_use]
    pub fn status_patches(&self) -> Vec<(String, Vec<IpAddr>)> {
        self.dispositions
            .iter()
            .filter_map(|(k, d)| match d {
                ServiceDisposition::Programmed { ingress_ips, .. } => {
                    Some((k.clone(), ingress_ips.clone()))
                }
                ServiceDisposition::Pending { .. } | ServiceDisposition::Invalid { .. } => None,
            })
            .collect()
    }

    /// Compute the observability snapshot for this reconcile — the gauges an
    /// operator watches (see [`ServiceLbMetrics`]).
    #[must_use]
    pub fn metrics(&self) -> ServiceLbMetrics {
        let mut m = ServiceLbMetrics::default();
        for (_, d) in &self.dispositions {
            m.services_total += 1;
            match d {
                ServiceDisposition::Programmed { ingress_ips, .. } => {
                    m.services_programmed += 1;
                    if !ingress_ips.is_empty() {
                        m.services_published += 1;
                    }
                }
                ServiceDisposition::Pending { conflicts } => {
                    m.services_pending += 1;
                    m.host_port_conflicts += conflicts.len();
                }
                ServiceDisposition::Invalid { .. } => m.services_invalid += 1,
            }
        }
        m.daemonsets_desired = self.apply.len();
        m.daemonsets_apply = self.apply.len();
        m.daemonsets_delete = self.delete.len();
        m
    }
}

/// A point-in-time observability snapshot of the ServiceLB controller.
///
/// Derived from one [`Reconciliation`] via [`Reconciliation::metrics`], these are
/// the operational signals the controller surfaces: how many
/// LoadBalancer Services exist and how they break down (programmed vs pending vs
/// invalid), how many actually publish an ingress IP (the "endpoint health"
/// line — a Service with a DaemonSet but no advertised node IP is still pending
/// from the caller's view), the svclb DaemonSet (pod-set) counts to apply /
/// delete, and the host-port conflict count.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ServiceLbMetrics {
    /// Total LoadBalancer Services seen this reconcile.
    pub services_total: usize,
    /// Services admitted with a svclb DaemonSet.
    pub services_programmed: usize,
    /// Services left pending by a host-port conflict.
    pub services_pending: usize,
    /// Services skipped by structural validation.
    pub services_invalid: usize,
    /// Programmed Services that publish at least one ingress IP (endpoint health).
    pub services_published: usize,
    /// svclb DaemonSets that should exist (== one per programmed Service).
    pub daemonsets_desired: usize,
    /// svclb DaemonSets to create-or-update this reconcile.
    pub daemonsets_apply: usize,
    /// Orphan svclb DaemonSets to delete this reconcile.
    pub daemonsets_delete: usize,
    /// Total host-port `(protocol, port)` conflicts across all pending Services.
    pub host_port_conflicts: usize,
}

impl ServiceLbMetrics {
    /// Render these gauges as Prometheus text exposition (one `HELP`/`TYPE`
    /// header + value line per gauge). All metrics are gauges except the
    /// conflict counter, which carries the conventional `_total` suffix.
    #[must_use]
    pub fn to_prometheus(&self) -> String {
        let gauges: [(&str, &str, usize); 8] = [
            (
                "servicelb_services_total",
                "LoadBalancer Services seen by the ServiceLB controller",
                self.services_total,
            ),
            (
                "servicelb_services_programmed",
                "Services admitted with a svclb DaemonSet",
                self.services_programmed,
            ),
            (
                "servicelb_services_pending",
                "Services left pending by a host-port conflict",
                self.services_pending,
            ),
            (
                "servicelb_services_invalid",
                "Services skipped by structural validation",
                self.services_invalid,
            ),
            (
                "servicelb_services_published",
                "Programmed Services publishing at least one ingress IP",
                self.services_published,
            ),
            (
                "servicelb_daemonsets_desired",
                "svclb DaemonSets that should exist",
                self.daemonsets_desired,
            ),
            (
                "servicelb_daemonsets_apply",
                "svclb DaemonSets to create-or-update",
                self.daemonsets_apply,
            ),
            (
                "servicelb_daemonsets_delete",
                "orphan svclb DaemonSets to delete",
                self.daemonsets_delete,
            ),
        ];

        let mut blocks: Vec<String> = gauges
            .iter()
            .map(|(name, help, value)| {
                format!("# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}")
            })
            .collect();
        // The one counter (conventional `_total` suffix).
        let c = "servicelb_host_port_conflicts_total";
        blocks.push(format!(
            "# HELP {c} host-port conflicts across pending Services\n# TYPE {c} counter\n{c} {}",
            self.host_port_conflicts
        ));
        let mut out = blocks.join("\n");
        out.push('\n');
        out
    }
}

/// Reconcile one cluster snapshot: decide the svclb DaemonSets to apply, the
/// orphans to delete, and every Service's disposition + ingress status.
///
/// `services` must be in a stable admission order (K3s effectively orders by the
/// Service's own identity / creation): that order decides who wins a contended
/// host port — the first claimant is programmed, later contenders are left
/// [`ServiceDisposition::Pending`]. The function is pure and deterministic: the
/// same snapshot always yields the same [`Reconciliation`].
#[must_use]
pub fn reconcile(
    services: &[LoadBalancerService],
    nodes: &[Node],
    ctx: &ReconcileContext,
) -> Reconciliation {
    let mut alloc = HostPortAllocator::new();
    let mut dispositions = Vec::with_capacity(services.len());
    let mut apply = Vec::new();
    let mut desired: BTreeSet<String> = BTreeSet::new();

    for svc in services {
        let key = svc.key();

        // 1. Structurally invalid Services are skipped — they claim no host port,
        //    so a later valid Service on the same port still programs.
        if let Err(reason) = validate_service(svc) {
            dispositions.push((key, ServiceDisposition::Invalid { reason }));
            continue;
        }

        // 2. Host-port admission: first-come wins; a contender is left pending.
        match alloc.allocate(svc) {
            AllocationOutcome::Conflicted { conflicts } => {
                dispositions.push((key, ServiceDisposition::Pending { conflicts }));
            }
            AllocationOutcome::Allocated { .. } => {
                // 3. Deploy: build the svclb pod spec (with DEST_IPS) + compute
                //    the ingress IPs this Service publishes.
                let dest = ctx.dest_ips.get(&key).cloned().unwrap_or_default();
                let spec = build_pod_spec(svc, &dest);
                let backing = ctx.backing_pods.get(&key).cloned().unwrap_or_default();
                let ingress_ips = compute_ingress_ips(svc, nodes, &backing);
                let daemonset = svc.svclb_daemonset_name();

                desired.insert(daemonset.clone());
                apply.push(spec);
                dispositions.push((
                    key,
                    ServiceDisposition::Programmed {
                        daemonset,
                        ingress_ips,
                    },
                ));
            }
        }
    }

    // 4. Orphans: any existing svclb DaemonSet not backing a programmed Service
    //    is deleted (Service removed / changed away from LoadBalancer / now
    //    pending). BTreeSet difference yields sorted, de-duplicated names.
    let delete: Vec<String> = ctx
        .existing_daemonsets
        .difference(&desired)
        .cloned()
        .collect();

    Reconciliation {
        dispositions,
        apply,
        delete,
    }
}
