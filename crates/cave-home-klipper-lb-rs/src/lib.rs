// SPDX-License-Identifier: Apache-2.0
//! `cave-home-klipper-lb-rs` — the ServiceLB ("svclb" / klipper-lb) decision
//! core for the cave-home K3s-style orchestration layer (ADR-004).
//!
//! # What this crate is
//!
//! K3s ships a built-in service load-balancer — *ServiceLB*, historically
//! *klipper-lb* — that provides `LoadBalancer`-type Services on bare metal
//! without a cloud provider. For every `LoadBalancer` Service it creates a
//! `DaemonSet` (`svclb-<service>`) whose pods bind the Service's ports on each
//! node's host network and forward them into the cluster Service.
//!
//! This crate is a **behavioural reimplementation of the documented svclb /
//! klipper-lb algorithm**: the host-port allocation + conflict detection, the
//! svclb pod-spec construction (the `SRC_PORT` / `DEST_PROTO` / `DEST_PORT` /
//! `DEST_IPS` env contract `entry.sh` consumes), the node selection, and the
//! `status.loadBalancer.ingress` computation. It is written from the *public*
//! K3s ServiceLB + Kubernetes Service API docs (see `parity.manifest.toml`
//! `spec_sources`), **not** a verbatim line-by-line transcription. Applying the
//! DaemonSet to a cluster, the in-pod iptables the container programs, and the
//! Service/Node informer + controller loop are ADR-justified Phase-1b work.
//!
//! This is **infrastructure** (Charter §6.3): it is hidden from end-users and
//! produces no user-facing strings — hence no i18n.
//!
//! # Layout
//!
//! * [`service`]    — the `LoadBalancerService` model + structural validation,
//! * [`node`]       — the cluster `Node` model + svclb node-selection,
//! * [`port_alloc`] — host-port allocation + cross-Service conflict detection,
//! * [`daemonset`]  — svclb pod-spec construction (per-port containers + env),
//! * [`status`]     — `status.loadBalancer.ingress` IP computation (Cluster /
//!   Local `externalTrafficPolicy`).
//!
//! # Example
//!
//! Allocate host ports for two Services contending for the same port, then
//! publish ingress IPs for the winner:
//!
//! ```
//! use std::collections::BTreeSet;
//! use std::net::IpAddr;
//! use cave_home_klipper_lb_rs::service::{
//!     ExternalTrafficPolicy, LoadBalancerService, Protocol, ServicePort,
//! };
//! use cave_home_klipper_lb_rs::node::Node;
//! use cave_home_klipper_lb_rs::port_alloc::HostPortAllocator;
//! use cave_home_klipper_lb_rs::status::compute_ingress_ips;
//!
//! let mk = |name: &str, np: u16| LoadBalancerService {
//!     namespace: "default".into(),
//!     name: name.into(),
//!     load_balancer_ips: vec![],
//!     ports: vec![ServicePort {
//!         name: "http".into(), protocol: Protocol::Tcp, port: 80, node_port: np,
//!     }],
//!     external_traffic_policy: ExternalTrafficPolicy::Cluster,
//!     node_selector: Default::default(),
//! };
//!
//! let web = mk("web", 30080);
//! let blog = mk("blog", 30081);
//!
//! let mut alloc = HostPortAllocator::new();
//! assert!(alloc.allocate(&web).is_allocated());
//! // blog wants the same host port 80 -> conflict, left pending.
//! assert!(!alloc.allocate(&blog).is_allocated());
//!
//! let internal: IpAddr = "10.0.0.1".parse().unwrap();
//! let nodes = vec![Node::new("n1").with_internal_ip(internal)];
//! let ingress = compute_ingress_ips(&web, &nodes, &BTreeSet::new());
//! assert_eq!(ingress, vec![internal]);
//! ```

pub mod controller;
pub mod daemonset;
pub mod node;
pub mod port_alloc;
pub mod service;
pub mod status;
