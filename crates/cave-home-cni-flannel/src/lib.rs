// SPDX-License-Identifier: Apache-2.0
//! `cave-home-cni-flannel` — the flannel CNI subnet-management and IPAM
//! *decision core* for cave-home's K3s pod networking (ADR-004 + ADR-008).
//!
//! # What this is
//!
//! flannel is a Container Network Interface (CNI) plugin: it gives every node
//! in a cluster a slice of a shared *pod CIDR*, assigns individual pod IPs out
//! of that slice, and programs the host so pods on different nodes can reach
//! each other. This crate implements the *decision logic* of that job, in pure
//! `std` (including [`std::net`]) with no async runtime, no kernel calls and no
//! network I/O:
//!
//! - [`cidr`] — CIDR arithmetic over [`std::net::IpAddr`] (v4 + v6):
//!   mask, containment, overlap, subnet splitting, nth-address.
//! - [`subnet`] — carve the cluster pod CIDR into per-node subnets and lease
//!   one per node; allocate, reserve, release, detect exhaustion.
//! - [`ipam`] — allocate and free individual pod IPs from a node subnet,
//!   reserving the network and gateway addresses.
//! - [`backend`] — typed flannel backend config (VXLAN / host-gw / `WireGuard`)
//!   and the per-node backend data peers advertise (VTEP MAC + public IP).
//! - [`routes`] — compute the routes and FDB entries a node must program to
//!   reach every peer subnet, given the node→subnet map and the backend.
//! - [`cni`] — model a CNI ADD (allocate + build result) and DEL (free)
//!   decision, returning the CNI result schema (IP, gateway, routes, DNS).
//!
//! # What is deferred
//!
//! The kernel datapath — bringing up the `flannel.<vni>` VXLAN device,
//! programming routes and the FDB via netlink, the `WireGuard` tunnel setup,
//! the long-running flannel daemon watch loop, and the durable subnet-lease
//! store (etcd / Kubernetes API) — is **not** in this crate. Those are the
//! I/O / privileged layers, deferred to Phase 1b and enumerated in
//! `parity.manifest.toml`. Everything here is the pure brain those layers
//! drive, and it is exercised entirely by unit tests.
//!
//! Per ADR-007 / Charter §6.3 this crate is *infrastructure*: it surfaces no
//! user-facing strings. The household never sees "CNI", "VXLAN" or "subnet".
//!
//! # Example
//!
//! Carve a cluster CIDR, lease two nodes their subnets, and assign a pod IP:
//!
//! ```
//! use std::str::FromStr;
//! use cave_home_cni_flannel::cidr::Cidr;
//! use cave_home_cni_flannel::subnet::SubnetManager;
//! use cave_home_cni_flannel::ipam::PodIpam;
//! use cave_home_cni_flannel::cni::cni_add;
//!
//! // Cluster pod network 10.42.0.0/16, one /24 per node (flannel defaults).
//! let cluster = Cidr::from_str("10.42.0.0/16")?;
//! let mut mgr = SubnetManager::new(cluster, 24)?;
//!
//! let node_a = mgr.allocate("node-a")?;
//! let node_b = mgr.allocate("node-b")?;
//! assert_eq!(node_a.subnet, Cidr::from_str("10.42.0.0/24")?);
//! assert_eq!(node_b.subnet, Cidr::from_str("10.42.1.0/24")?);
//!
//! // On node-a, hand a pod its first usable address (.0 + .1 are reserved).
//! let mut ipam = PodIpam::new(node_a.subnet)?;
//! let result = cni_add(&mut ipam, &[])?;
//! assert_eq!(result.ip.address_cidr_string(), "10.42.0.2/24");
//! assert_eq!(result.ip.gateway.to_string(), "10.42.0.1");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod backend;
pub mod cidr;
pub mod cni;
pub mod datapath;
pub mod device;
pub mod ipam;
pub mod mac;
pub mod netlink;
pub mod route_network;
pub mod routes;
pub mod subnet;
pub mod vxlan_network;
