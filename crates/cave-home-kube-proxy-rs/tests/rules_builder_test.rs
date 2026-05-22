// SPDX-License-Identifier: Apache-2.0
//! Snapshot tests for `build_proxy_rules`. Each fixture is harvested from
//! upstream `pkg/proxy/iptables/proxier_test.go` *exact* expected output
//! lines (only the ClusterIP-relevant subset is asserted — NodePort,
//! LoadBalancer, ExternalIP, conntrack-cleanup, mark-masq postrouting are
//! Phase 1b and intentionally skipped).
//!
//! The tests assert the lines our builder emits are a SUBSET of the
//! upstream snapshot, in the SAME relative order. A strict full-equality
//! comparison would fail on Phase 1b lines we deliberately don't emit yet.

use cave_home_kube_proxy_rs::api::{NamespacedName, Protocol, ServicePortName};
use cave_home_kube_proxy_rs::iptables::rules_builder::{
    build_proxy_rules, BuildInput,
};
use cave_home_kube_proxy_rs::iptables::types::{EndpointInfo, ServicePortInfo, Table};

fn spn(ns: &str, name: &str, port: &str) -> ServicePortName {
    ServicePortName {
        namespaced_name: NamespacedName::new(ns, name),
        port: port.into(),
        protocol: Protocol::Tcp,
    }
}

#[test]
fn emits_nat_table_header_and_commit() {
    let input = BuildInput {
        services: Vec::new(),
        endpoints_by_service: std::collections::BTreeMap::new(),
        cluster_cidr: Some("10.0.0.0/24".into()),
    };
    let rules = build_proxy_rules(&input);
    let nat_text = render(&rules, Table::Nat);
    assert!(nat_text.starts_with("*nat\n"), "got:\n{nat_text}");
    assert!(nat_text.trim_end().ends_with("COMMIT"), "got:\n{nat_text}");
}

#[test]
fn emits_kube_services_chain_declaration() {
    let input = BuildInput {
        services: Vec::new(),
        endpoints_by_service: std::collections::BTreeMap::new(),
        cluster_cidr: None,
    };
    let nat = render(&build_proxy_rules(&input), Table::Nat);
    assert!(
        nat.contains(":KUBE-SERVICES - [0:0]"),
        "missing KUBE-SERVICES decl in:\n{nat}"
    );
}

#[test]
fn emits_kube_postrouting_skeleton() {
    // Upstream lines 205-208 (proxier_test.go):
    //   :KUBE-POSTROUTING - [0:0]
    //   :KUBE-MARK-MASQ - [0:0]
    //   -A KUBE-POSTROUTING -m mark ! --mark 0x4000/0x4000 -j RETURN
    //   -A KUBE-POSTROUTING -j MARK --xor-mark 0x4000
    //   -A KUBE-POSTROUTING -m comment --comment "kubernetes service traffic requiring SNAT" -j MASQUERADE
    //   -A KUBE-MARK-MASQ -j MARK --or-mark 0x4000
    let input = BuildInput {
        services: Vec::new(),
        endpoints_by_service: std::collections::BTreeMap::new(),
        cluster_cidr: None,
    };
    let nat = render(&build_proxy_rules(&input), Table::Nat);
    for needle in [
        ":KUBE-POSTROUTING - [0:0]",
        ":KUBE-MARK-MASQ - [0:0]",
        "-A KUBE-POSTROUTING -m mark ! --mark 0x4000/0x4000 -j RETURN",
        "-A KUBE-POSTROUTING -j MARK --xor-mark 0x4000",
        "-A KUBE-POSTROUTING -m comment --comment \"kubernetes service traffic requiring SNAT\" -j MASQUERADE",
        "-A KUBE-MARK-MASQ -j MARK --or-mark 0x4000",
    ] {
        assert!(nat.contains(needle), "missing `{needle}` in:\n{nat}");
    }
}

// ---------------------------------------------------------------------------
// Upstream proxier_test.go fixture (lines 195-214):  ns1/svc1:p80 (ClusterIP
// 10.20.30.41) -> single endpoint 10.180.0.1:80, cluster-cidr 10.0.0.0/24.
// ---------------------------------------------------------------------------

fn fixture_one_svc_one_endpoint() -> BuildInput {
    let svc_name = spn("ns1", "svc1", "p80");
    let mut eps = std::collections::BTreeMap::new();
    eps.insert(
        svc_name.clone(),
        vec![EndpointInfo { ip: "10.180.0.1".into(), port: 80 }],
    );
    BuildInput {
        services: vec![ServicePortInfo {
            name: svc_name,
            cluster_ip: "10.20.30.41".into(),
            port: 80,
            protocol: Protocol::Tcp,
        }],
        endpoints_by_service: eps,
        cluster_cidr: Some("10.0.0.0/24".into()),
    }
}

#[test]
fn emits_kube_svc_chain_declaration_for_each_service() {
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(":KUBE-SVC-XPGD46QRK7WJZT7O - [0:0]"));
}

#[test]
fn emits_kube_sep_chain_declaration_for_each_endpoint() {
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(":KUBE-SEP-SXIVWICOYRO3J4NJ - [0:0]"));
}

#[test]
fn emits_clusterip_dispatch_rule_in_kube_services() {
    // Upstream line 209.
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(
        "-A KUBE-SERVICES -m comment --comment \"ns1/svc1:p80 cluster IP\" \
         -m tcp -p tcp -d 10.20.30.41 --dport 80 -j KUBE-SVC-XPGD46QRK7WJZT7O"
    ), "got:\n{nat}");
}

#[test]
fn emits_cluster_egress_masquerade_mark_when_cluster_cidr_set() {
    // Upstream line 210: when cluster_cidr is set, traffic NOT from the
    // cluster CIDR going to the ClusterIP is marked for masquerade.
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(
        "-A KUBE-SVC-XPGD46QRK7WJZT7O -m comment --comment \"ns1/svc1:p80 cluster IP\" \
         -m tcp -p tcp -d 10.20.30.41 --dport 80 ! -s 10.0.0.0/24 -j KUBE-MARK-MASQ"
    ), "got:\n{nat}");
}

#[test]
fn emits_dispatch_to_sep_chain_in_svc_chain() {
    // Upstream line 211.
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(
        "-A KUBE-SVC-XPGD46QRK7WJZT7O -m comment --comment ns1/svc1:p80 \
         -j KUBE-SEP-SXIVWICOYRO3J4NJ"
    ), "got:\n{nat}");
}

#[test]
fn emits_endpoint_self_masquerade_rule() {
    // Upstream line 212: hairpin/loopback masquerade when source IP is the
    // endpoint itself.
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(
        "-A KUBE-SEP-SXIVWICOYRO3J4NJ -m comment --comment ns1/svc1:p80 \
         -s 10.180.0.1 -j KUBE-MARK-MASQ"
    ), "got:\n{nat}");
}

#[test]
fn emits_dnat_rule_to_endpoint() {
    // Upstream line 213.
    let nat = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert!(nat.contains(
        "-A KUBE-SEP-SXIVWICOYRO3J4NJ -m comment --comment ns1/svc1:p80 \
         -m tcp -p tcp -j DNAT --to-destination 10.180.0.1:80"
    ), "got:\n{nat}");
}

// ---------------------------------------------------------------------------
// N>1 endpoint distribution: --mode random --probability 1/N for the first
// N-1 endpoints, no probability on the last one (upstream behaviour, see
// `pkg/proxy/iptables/proxier.go syncProxyRules` endpoints loop).
// ---------------------------------------------------------------------------

#[test]
fn three_endpoints_emit_two_probability_rules_then_unconditional() {
    let svc_name = spn("ns1", "svc1", "p80");
    let mut eps = std::collections::BTreeMap::new();
    eps.insert(
        svc_name.clone(),
        vec![
            EndpointInfo { ip: "10.180.0.1".into(), port: 80 },
            EndpointInfo { ip: "10.180.0.2".into(), port: 80 },
            EndpointInfo { ip: "10.180.0.3".into(), port: 80 },
        ],
    );
    let input = BuildInput {
        services: vec![ServicePortInfo {
            name: svc_name,
            cluster_ip: "10.20.30.41".into(),
            port: 80,
            protocol: Protocol::Tcp,
        }],
        endpoints_by_service: eps,
        cluster_cidr: None,
    };
    let nat = render(&build_proxy_rules(&input), Table::Nat);

    // First (1/3) and second (1/2) get -m statistic; third unconditional.
    assert!(nat.contains("-m statistic --mode random --probability 0.3333333333"),
        "expected 1/3 prob in:\n{nat}");
    assert!(nat.contains("-m statistic --mode random --probability 0.5000000000"),
        "expected 1/2 prob in:\n{nat}");
    // The last endpoint dispatch line MUST NOT carry --probability.
    let last_line = nat.lines()
        .filter(|l| l.contains("-A KUBE-SVC-") && l.contains("-j KUBE-SEP-"))
        .last()
        .unwrap_or("");
    assert!(!last_line.contains("--probability"),
        "last endpoint line must be unconditional, got: {last_line}");
}

#[test]
fn endpoints_emitted_in_sorted_order_for_determinism() {
    // Upstream syncProxyRules sorts endpoints; we hand the builder an
    // intentionally unsorted list and assert the output is sorted by IP+port.
    let svc_name = spn("ns1", "svc1", "p80");
    let mut eps = std::collections::BTreeMap::new();
    eps.insert(
        svc_name.clone(),
        vec![
            EndpointInfo { ip: "10.180.0.3".into(), port: 80 },
            EndpointInfo { ip: "10.180.0.1".into(), port: 80 },
            EndpointInfo { ip: "10.180.0.2".into(), port: 80 },
        ],
    );
    let input = BuildInput {
        services: vec![ServicePortInfo {
            name: svc_name,
            cluster_ip: "10.20.30.41".into(),
            port: 80,
            protocol: Protocol::Tcp,
        }],
        endpoints_by_service: eps,
        cluster_cidr: None,
    };
    let nat = render(&build_proxy_rules(&input), Table::Nat);

    let ip_order: Vec<&str> = nat.lines()
        .filter(|l| l.contains("--to-destination"))
        .filter_map(|l| l.split("--to-destination ").nth(1))
        .collect();
    assert_eq!(ip_order, vec!["10.180.0.1:80", "10.180.0.2:80", "10.180.0.3:80"]);
}

#[test]
fn services_emitted_in_sorted_order_for_determinism() {
    let mut eps = std::collections::BTreeMap::new();
    eps.insert(spn("ns1", "svc1", "p80"), vec![EndpointInfo { ip: "10.180.0.1".into(), port: 80 }]);
    eps.insert(spn("ns2", "svc2", "p80"), vec![EndpointInfo { ip: "10.180.2.1".into(), port: 80 }]);

    let input = BuildInput {
        services: vec![
            // Pass services in reverse order — builder must sort them.
            ServicePortInfo {
                name: spn("ns2", "svc2", "p80"),
                cluster_ip: "10.20.30.42".into(), port: 80, protocol: Protocol::Tcp,
            },
            ServicePortInfo {
                name: spn("ns1", "svc1", "p80"),
                cluster_ip: "10.20.30.41".into(), port: 80, protocol: Protocol::Tcp,
            },
        ],
        endpoints_by_service: eps,
        cluster_cidr: None,
    };
    let nat = render(&build_proxy_rules(&input), Table::Nat);
    let svc_first = nat.find("ns1/svc1:p80 cluster IP").unwrap_or(usize::MAX);
    let svc_second = nat.find("ns2/svc2:p80 cluster IP").unwrap_or(0);
    assert!(svc_first < svc_second, "ns1 must come before ns2:\n{nat}");
}

#[test]
fn build_is_deterministic_across_runs() {
    let a = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    let b = render(&build_proxy_rules(&fixture_one_svc_one_endpoint()), Table::Nat);
    assert_eq!(a, b);
}

#[test]
fn no_cluster_cidr_omits_egress_masquerade_rule() {
    // Upstream syncProxyRules: the `! -s <cluster-cidr> -j KUBE-MARK-MASQ`
    // rule on KUBE-SVC-XXXX is only emitted when --cluster-cidr is set.
    let mut input = fixture_one_svc_one_endpoint();
    input.cluster_cidr = None;
    let nat = render(&build_proxy_rules(&input), Table::Nat);
    assert!(
        !nat.contains("! -s 10.0.0.0/24 -j KUBE-MARK-MASQ"),
        "no cluster CIDR ⇒ no egress masq rule:\n{nat}"
    );
    // But the cluster-IP dispatch rule still appears.
    assert!(nat.contains("-A KUBE-SERVICES"));
}

#[test]
fn endpoints_with_no_endpoints_emit_reject_or_no_dispatch() {
    // When a service has zero ready endpoints, upstream emits NO dispatch
    // line in KUBE-SVC- (the chain itself is still declared). Phase 1
    // matches that — no REJECT yet (Phase 1b).
    let svc_name = spn("ns1", "svc1", "p80");
    let mut eps = std::collections::BTreeMap::new();
    eps.insert(svc_name.clone(), Vec::new());
    let input = BuildInput {
        services: vec![ServicePortInfo {
            name: svc_name, cluster_ip: "10.20.30.41".into(), port: 80, protocol: Protocol::Tcp,
        }],
        endpoints_by_service: eps,
        cluster_cidr: None,
    };
    let nat = render(&build_proxy_rules(&input), Table::Nat);
    assert!(nat.contains(":KUBE-SVC-XPGD46QRK7WJZT7O - [0:0]"));
    assert!(!nat.contains("-A KUBE-SVC-XPGD46QRK7WJZT7O -m comment --comment ns1/svc1:p80 -j KUBE-SEP-"),
        "no SEP chains expected when there are no endpoints:\n{nat}");
}

// --- helper -----------------------------------------------------------------

fn render(rules: &[cave_home_kube_proxy_rs::iptables::types::IptablesRule], table: Table) -> String {
    rules.iter()
        .filter(|r| r.table == table)
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n") + "\n"
}
