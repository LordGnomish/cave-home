// SPDX-License-Identifier: Apache-2.0
//! `cave-home-kube-proxy-rs` — the service-proxy data-plane decision core for
//! the cave-home K3s-style orchestration layer (ADR-004).
//!
//! # What this crate is
//!
//! A **behavioural reimplementation of the documented kube-proxy algorithm**:
//! it turns the cluster's `Service` + `EndpointSlice` objects into packet-
//! forwarding rules. It is written from the *public* Kubernetes API + kube-proxy
//! documentation (see `parity.manifest.toml` `spec_sources`), not as a verbatim
//! line-by-line transcription of any specific upstream source revision — the
//! verbatim-parity remainder is tracked as ADR-justified deferred work.
//!
//! This is **infrastructure** (Charter §6.3): it is hidden from end-users and
//! produces no user-facing strings.
//!
//! # Layout
//!
//! * [`core`] — backend-agnostic decision core:
//!   * [`core::model`] — Service / EndpointSlice data model,
//!   * [`core::validate`] — structural validation,
//!   * [`core::select`] — endpoint selection (readiness, topology hints,
//!     `externalTrafficPolicy`, session affinity),
//!   * [`core::rules`] — the `ProxyRule` IR, rule generation, and the
//!     incremental add/remove diff.
//! * [`iptables`] — lowering of the decision to `iptables-restore` text plus a
//!   Linux executor (the one wire backend implemented so far).
//! * [`cache`] / [`proxier`] — Service/EndpointSlice caches and the debounced
//!   reconciler loop that drives a sync.
//!
//! # Example
//!
//! Build forwarding rules for a one-port ClusterIP Service with one ready
//! endpoint:
//!
//! ```
//! use std::net::IpAddr;
//! use cave_home_kube_proxy_rs::core::model::{
//!     Endpoint, EndpointConditions, EndpointPort, EndpointSlice, Protocol,
//!     Service, ServicePort, ServiceType, SessionAffinity, ExternalTrafficPolicy,
//! };
//! use cave_home_kube_proxy_rs::core::rules::{build_rules, ServiceInput, RuleAction};
//! use cave_home_kube_proxy_rs::core::select::NodeContext;
//!
//! let svc = Service {
//!     namespace: "ns1".into(),
//!     name: "web".into(),
//!     cluster_ip: Some("10.96.0.10".parse::<IpAddr>().unwrap()),
//!     service_type: ServiceType::ClusterIp,
//!     ports: vec![ServicePort {
//!         name: "http".into(), protocol: Protocol::Tcp, port: 80,
//!         target_port: 8080, node_port: None,
//!     }],
//!     session_affinity: SessionAffinity::None,
//!     external_traffic_policy: ExternalTrafficPolicy::Cluster,
//!     load_balancer_ips: vec![],
//! };
//! let slice = EndpointSlice {
//!     namespace: "ns1".into(),
//!     service_name: "web".into(),
//!     slice_name: "web-abc".into(),
//!     ports: vec![EndpointPort { name: "http".into(), protocol: Protocol::Tcp, port: 8080 }],
//!     endpoints: vec![Endpoint {
//!         addresses: vec!["10.1.0.5".parse().unwrap()],
//!         conditions: EndpointConditions::default(),
//!         node_name: None, zone: None, hints_for_zones: vec![],
//!     }],
//! };
//!
//! let inputs = [ServiceInput { service: &svc, slices: std::slice::from_ref(&slice) }];
//! let rules = build_rules(&inputs, &NodeContext::default());
//! assert_eq!(rules.len(), 1);
//! match &rules[0].action {
//!     RuleAction::Forward { backends, .. } => assert_eq!(backends.len(), 1),
//!     RuleAction::Reject => panic!("expected a forward rule"),
//! }
//! ```

pub mod api;
pub mod cache;
pub mod core;
pub mod iptables;
pub mod proxier;
