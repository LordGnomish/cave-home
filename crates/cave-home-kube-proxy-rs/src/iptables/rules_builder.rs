// SPDX-License-Identifier: Apache-2.0
//! Pure rule generator — line-by-line port of the ClusterIP path of
//! upstream `pkg/proxy/iptables/proxier.go syncProxyRules` (lines ~1200-1900).
//!
//! Phase 1 emits ONLY the ClusterIP / KUBE-SERVICES → KUBE-SVC- → KUBE-SEP-
//! dispatch chain, plus the static KUBE-POSTROUTING / KUBE-MARK-MASQ
//! skeleton. NodePort / LoadBalancer / ExternalIP / ExternalTrafficPolicy
//! / InternalTrafficPolicy / IPVS / nftables / IPv6 / topology hints are
//! deferred to Phase 1b — see `parity.manifest.toml`.

use std::collections::BTreeMap;

use crate::api::{Protocol, ServicePortName};
use crate::iptables::chain_names::{
    service_port_endpoint_chain_name, service_port_policy_cluster_chain,
};
use crate::iptables::types::{EndpointInfo, IptablesRule, ServicePortInfo, Table};

/// Builder input — already-snapshotted view of the world. Cache layer
/// produces this; reconciler hands it to `build_proxy_rules`.
#[derive(Debug, Clone, Default)]
pub struct BuildInput {
    pub services: Vec<ServicePortInfo>,
    /// Endpoints keyed by the same `ServicePortName` as the matching
    /// `ServicePortInfo.name`. Missing key == 0 endpoints (chain still
    /// declared, no dispatch).
    pub endpoints_by_service: BTreeMap<ServicePortName, Vec<EndpointInfo>>,
    /// `--cluster-cidr` flag from kube-proxy. When `None`, the
    /// per-service "cluster-egress masquerade" rule is omitted.
    pub cluster_cidr: Option<String>,
}

/// Upstream `syncProxyRules` core: returns a deterministic list of
/// `IptablesRule` lines ready to feed to `iptables-restore`.
///
/// Determinism guarantees:
/// - Services sorted by `ServicePortName` (namespace, name, port).
/// - Endpoints within a service sorted by (ip, port).
/// - Output identical across runs for identical input.
#[must_use]
pub fn build_proxy_rules(input: &BuildInput) -> Vec<IptablesRule> {
    let mut out = Vec::with_capacity(64);
    let nat = |text: String| IptablesRule { table: Table::Nat, text };

    // -- *nat header ----------------------------------------------------------
    out.push(nat("*nat".into()));

    // -- top-level chain declarations -----------------------------------------
    // Upstream proxier_test.go lines 199-203.
    out.push(nat(":KUBE-SERVICES - [0:0]".into()));
    out.push(nat(":KUBE-NODEPORTS - [0:0]".into()));
    out.push(nat(":KUBE-POSTROUTING - [0:0]".into()));
    out.push(nat(":KUBE-MARK-MASQ - [0:0]".into()));

    // Sort services for deterministic iteration.
    let mut services_sorted = input.services.clone();
    services_sorted.sort_by(|a, b| a.name.cmp(&b.name));

    // -- per-service chain declarations (KUBE-SVC- + KUBE-SEP-) ---------------
    // Upstream emits these BEFORE the rule body (lines 204-205 in fixture).
    for svc in &services_sorted {
        let svc_chain = service_port_policy_cluster_chain(
            &svc.name.to_string(),
            svc.protocol.lowercase(),
        );
        out.push(nat(format!(":{svc_chain} - [0:0]")));

        let mut endpoints = input
            .endpoints_by_service
            .get(&svc.name)
            .cloned()
            .unwrap_or_default();
        endpoints.sort();
        for ep in &endpoints {
            let sep_chain = service_port_endpoint_chain_name(
                &svc.name.to_string(),
                svc.protocol.lowercase(),
                &ep.endpoint_string(),
            );
            out.push(nat(format!(":{sep_chain} - [0:0]")));
        }
    }

    // -- KUBE-POSTROUTING + KUBE-MARK-MASQ skeleton ---------------------------
    // Upstream proxier.go writeIPTablesRules — static every sync.
    out.push(nat(
        "-A KUBE-POSTROUTING -m mark ! --mark 0x4000/0x4000 -j RETURN".into(),
    ));
    out.push(nat("-A KUBE-POSTROUTING -j MARK --xor-mark 0x4000".into()));
    out.push(nat(
        "-A KUBE-POSTROUTING -m comment --comment \"kubernetes service traffic requiring SNAT\" \
         -j MASQUERADE"
            .into(),
    ));
    out.push(nat("-A KUBE-MARK-MASQ -j MARK --or-mark 0x4000".into()));

    // -- per-service rule body ------------------------------------------------
    for svc in &services_sorted {
        emit_service_rules(&mut out, svc, input, &nat_factory);
    }

    // -- KUBE-NODEPORTS jump trailer (NodePort routing itself = Phase 1b) -----
    // Upstream always emits the LOCAL-addrtype jump as the LAST KUBE-SERVICES
    // rule (proxier_test.go line 214).
    out.push(nat(
        "-A KUBE-SERVICES -m comment --comment \"kubernetes service nodeports; \
         NOTE: this must be the last rule in this chain\" \
         -m addrtype --dst-type LOCAL -j KUBE-NODEPORTS"
            .into(),
    ));

    // -- COMMIT ---------------------------------------------------------------
    out.push(nat("COMMIT".into()));

    out
}

/// Constructor for the local `nat` rule helper — kept as a separate fn so
/// `emit_service_rules` can borrow it without re-capturing closures.
const fn nat_factory(text: String) -> IptablesRule {
    IptablesRule { table: Table::Nat, text }
}

fn emit_service_rules(
    out: &mut Vec<IptablesRule>,
    svc: &ServicePortInfo,
    input: &BuildInput,
    nat: &impl Fn(String) -> IptablesRule,
) {
    let proto = svc.protocol.lowercase();
    let spn_str = svc.name.to_string();
    let svc_chain = service_port_policy_cluster_chain(&spn_str, proto);

    // -- ClusterIP dispatch in KUBE-SERVICES ---------------------------------
    // Upstream line 209.
    out.push(nat(format!(
        "-A KUBE-SERVICES -m comment --comment \"{spn_str} cluster IP\" \
         -m {proto} -p {proto} -d {clusterip} --dport {port} -j {svc_chain}",
        clusterip = svc.cluster_ip,
        port = svc.port,
    )));

    // -- Cluster-egress masquerade-mark (only when --cluster-cidr is set) ----
    // Upstream line 210.
    if let Some(cidr) = &input.cluster_cidr {
        out.push(nat(format!(
            "-A {svc_chain} -m comment --comment \"{spn_str} cluster IP\" \
             -m {proto} -p {proto} -d {clusterip} --dport {port} \
             ! -s {cidr} -j KUBE-MARK-MASQ",
            clusterip = svc.cluster_ip,
            port = svc.port,
        )));
    }

    // -- per-endpoint dispatch + SEP-chain bodies ----------------------------
    let mut endpoints = input
        .endpoints_by_service
        .get(&svc.name)
        .cloned()
        .unwrap_or_default();
    endpoints.sort();
    let n = endpoints.len();
    for (i, ep) in endpoints.iter().enumerate() {
        let sep_chain = service_port_endpoint_chain_name(&spn_str, proto, &ep.endpoint_string());
        // Dispatch line: probability 1/(N-i) for first N-1, unconditional for last.
        let remaining = n - i;
        let dispatch = if remaining > 1 {
            // Upstream `precomputeProbabilities` formats with 10 decimals.
            #[allow(clippy::cast_precision_loss)]
            let p = 1.0_f64 / (remaining as f64);
            format!(
                "-A {svc_chain} -m comment --comment {spn_str} \
                 -m statistic --mode random --probability {p:.10} \
                 -j {sep_chain}"
            )
        } else {
            format!("-A {svc_chain} -m comment --comment {spn_str} -j {sep_chain}")
        };
        out.push(nat(dispatch));

        // SEP body: hairpin masquerade then DNAT.
        out.push(nat(format!(
            "-A {sep_chain} -m comment --comment {spn_str} -s {ip} -j KUBE-MARK-MASQ",
            ip = ep.ip,
        )));
        out.push(nat(format!(
            "-A {sep_chain} -m comment --comment {spn_str} -m {proto} -p {proto} \
             -j DNAT --to-destination {ip}:{port}",
            ip = ep.ip,
            port = ep.port,
        )));
    }

    // Suppress unused-protocol enum warning when only Tcp arms used today.
    let _ = Protocol::Sctp;
}
