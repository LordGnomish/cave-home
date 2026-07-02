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
//! - [`neigh`] — compute the permanent ARP/NDP proxy-neighbor entries the
//!   VXLAN overlay device needs to resolve each peer VTEP IP to its MAC.
//! - [`cni`] — model a CNI ADD (allocate + build result) and DEL (free)
//!   decision, returning the CNI result schema (IP, gateway, routes, DNS).
//! - [`dualstack`] — pair the v4 and v6 subnet managers / pod IPAMs so a node
//!   leases both families and a pod gets both addresses, atomically.
//!
//! # What is deferred
//! # The real network backend
//!
//! The decision core above is *driven* by the real datapath, also in this
//! crate (the 2026-06-07 real-network port):
//!
//! - [`netlink`] — the rtnetlink wire codec (the bytes the kernel expects).
//! - [`datapath`] — the [`datapath::Datapath`] seam + a recording mock.
//! - [`device`] — the `flannel.<vni>` VXLAN device (create / address / FDB / ARP).
//! - [`vxlan_network`] / [`route_network`] / [`hostgw`] — turn a peer lease into
//!   real ARP/FDB/route state (VXLAN) or a direct route (host-gw).
//! - [`netlink_socket`] — the live `AF_NETLINK` socket (Linux) behind the seam.
//! - [`subnet_registry`] — the etcd/kine subnet-lease store + watch→event bridge.
//! - [`subnet_env`] / [`cni_delegate`] — the `subnet.env` contract and the CNI
//!   bridge delegate the `/opt/cni/bin/flannel` plugin emits.
//!
//! What is still deferred (see `parity.manifest.toml`): the `WireGuard`
//! genetlink datapath, the long-running daemon watch-loop wiring, the IPv6
//! VXLAN datapath, and CNI delegate exec/chaining.
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
pub mod cni_delegate;
pub mod datapath;
pub mod device;
pub mod dualstack;
pub mod hostgw;
pub mod ipam;
pub mod mac;
pub mod neigh;
pub mod netlink;
pub mod netlink_socket;
pub mod route_network;
pub mod routes;
pub mod subnet;
pub mod subnet_env;
pub mod subnet_registry;
pub mod vxlan_network;
