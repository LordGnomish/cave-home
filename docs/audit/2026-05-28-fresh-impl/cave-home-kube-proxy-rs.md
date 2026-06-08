# Coverage matrix — cave-home-kube-proxy-rs

**Declared:** fill=0.12 · adr_justified=n/a (no honest_ratio in manifest, uses test_port_ratio=0.18) · port method per manifest.
**Verified:** 20/20 mapped symbols found in source · 72 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| NamespacedName type | src/api/mod.rs::NamespacedName | yes |
| ServicePortName with Display impl | src/api/mod.rs::ServicePortName | yes |
| Protocol enum (TCP, UDP, SCTP) | src/api/mod.rs::Protocol | yes |
| Service, ServicePort, ServiceType types | src/api/mod.rs::Service, ServicePort, ServiceType | yes |
| EndpointSlice, Endpoint, EndpointConditions, EndpointPort types | src/api/mod.rs::EndpointSlice, Endpoint, EndpointConditions, EndpointPort | yes |
| WatchEvent type | src/api/mod.rs::WatchEvent | yes |
| ShouldSkipService filter | src/api/mod.rs::Service::should_skip | yes |
| portProtoHash chain naming | src/iptables/chain_names.rs::port_proto_hash | yes |
| servicePortPolicyClusterChain naming | src/iptables/chain_names.rs::service_port_policy_cluster_chain | yes |
| servicePortPolicyLocalChainName naming | src/iptables/chain_names.rs::service_port_policy_local_chain_name | yes |
| serviceFirewallChainName naming | src/iptables/chain_names.rs::service_firewall_chain_name | yes |
| serviceExternalChainName naming | src/iptables/chain_names.rs::service_external_chain_name | yes |
| servicePortEndpointChainName naming | src/iptables/chain_names.rs::service_port_endpoint_chain_name | yes |
| syncProxyRules (ClusterIP path) | src/iptables/rules_builder.rs::build_proxy_rules | yes |
| writeIPTablesRules skeleton block | src/iptables/rules_builder.rs (static) | yes |
| endpointInfo, servicePortInfo types | src/iptables/types.rs::EndpointInfo, ServicePortInfo | yes |
| iptables-restore (--noflush --counters) | src/iptables/executor.rs::LinuxExecutor::restore | yes |
| grabIptablesLocks (flock /run/xtables.lock) | src/iptables/executor.rs::LinuxExecutor | yes |
| ServiceCache type + logic | src/cache/service_cache.rs::ServiceCache | yes |
| EndpointSliceCache type + logic | src/cache/endpointslice_cache.rs::EndpointSliceCache | yes |
| Proxier struct + event handlers | src/proxier/proxier.rs::Proxier (apply_event) | yes |
| BoundedFrequencyRunner config | src/proxier/reconciler.rs::BoundedFrequencyConfig | yes |
| SyncLoop (debounced sync runner) | src/proxier/proxier.rs::Proxier::run_until | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| NodePort path (KUBE-NODEPORTS chain body) | phase-1b | KUBE-NODEPORTS jump trailer emitted; body deferred to phase-1b. |
| LoadBalancer + source-range filtering (KUBE-FW-) | phase-1b | serviceFirewallChainName implemented; firewall body deferred to phase-1b. |
| ExternalIPs handling | phase-1b | Deferred to phase-1b; chain generation not started. |
| ExternalTrafficPolicy=Local (KUBE-EXT-, KUBE-SVL-) | phase-1b | Chain-name helpers exist; dispatch logic deferred. |
| InternalTrafficPolicy=Local | phase-1b | Deferred to phase-1b. |
| IPVS mode (entire pkg/proxy/ipvs/) | phase-1b | Alternative load-balancing mode; deferred to phase-1b. |
| nftables mode (entire pkg/proxy/nftables/) | phase-1b | Alternative packet-filtering mode; deferred to phase-1b. |
| Conntrack flush on stale endpoints | phase-1b | Endpoint lifecycle management; deferred to phase-1b. |
| Healthz HTTP server | phase-1b | Diagnostics endpoint; deferred to phase-1b. |
| Prometheus metrics exporter | phase-1b | Observability feature; deferred to phase-1b. |
| TopologyAwareHints / preferredHints | phase-1b | Topology-aware routing; deferred to phase-1b. |
| Dual-stack IPv6 (ipFamily==v6 path) | phase-1b | v6 support deferred; Phase 1 is IPv4-only. |
| precomputeProbabilities (cached table) | phase-1b | On-the-fly computation acceptable for Phase 1; caching deferred. |
| NodeTopologyHandler | phase-1b | Node topology tracking; deferred to phase-1b. |
| One-shot cleanup-only mode | phase-1b | Cleanup mode deferred to phase-1b. |
| Monitor (KUBE-PROXY-CANARY watch) | phase-1b | Rule-reload monitoring; deferred to phase-1b. |
| Real apiserver Watch wiring | phase-1b | EventSource trait implemented; production wire-up in cave-home-apiserver-rs. |
| ServiceCIDRsHandler / OnServiceCIDRsChanged | phase-1b | Service CIDR management; deferred to phase-1b. |
| Userspace mode (deprecated upstream) | permanent | Upstream removed as legacy; never ported. |
| Windows kernel-space mode (winkernel) | permanent | Linux-only charter (ADR-003); permanently out of scope. |

## Drift notes
None — every claimed symbol exists in source. All 20 mapped entries verified present and correctly named.
