// SPDX-License-Identifier: Apache-2.0
//! Two-node simulated network: cross-node pod-to-pod routing.
//!
//! This is the integration test the real-network port exists for. It builds a
//! two-node flannel cluster entirely against the [`MockDatapath`] netlink seam,
//! lets each node lease its pod subnet, bring up its `flannel.1` VXLAN device
//! and program the path to its peer, and then *simulates a packet* between pods
//! on different nodes by walking the kernel state each node actually programmed
//! (route table → ARP → FDB → underlay endpoint). It asserts the encapsulated
//! frame is delivered to the correct peer and decapsulates to the right pod —
//! i.e. that the datapath this crate programs would really carry pod traffic.
//!
//! It also runs the host-gw equivalent (direct routes, no encapsulation).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

use cave_home_cni_flannel::backend::{MacAddr, NodeBackendData, VxlanConfig};
use cave_home_cni_flannel::cidr::Cidr;
use cave_home_cni_flannel::datapath::{MockDatapath, Op, Route};
use cave_home_cni_flannel::device::{VxlanDevice, VxlanDeviceAttrs};
use cave_home_cni_flannel::hostgw::{ExternalInterface, HostGwBackend};
use cave_home_cni_flannel::ipam::PodIpam;
use cave_home_cni_flannel::routes::PeerLease;
use cave_home_cni_flannel::subnet::SubnetManager;
use cave_home_cni_flannel::vxlan_network::{LeaseEvent, VxlanNetwork};

fn v4(s: &str) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
}
fn cidr(s: &str) -> Cidr {
    Cidr::from_str(s).expect("cidr")
}

/// The kernel state one node programmed, reconstructed from the recorded ops:
/// the route table, the overlay ARP table (gateway IP → VTEP MAC) and the FDB
/// (VTEP MAC → underlay endpoint). This is exactly what the Linux kernel would
/// consult to forward and encapsulate an overlay packet.
struct NodeKernel {
    subnet: Cidr,
    routes: Vec<Route>,
    arp: HashMap<IpAddr, MacAddr>,
    fdb: HashMap<MacAddr, IpAddr>,
}

impl NodeKernel {
    fn from_ops(subnet: Cidr, ops: &[Op]) -> Self {
        let mut routes = Vec::new();
        let mut arp = HashMap::new();
        let mut fdb = HashMap::new();
        for op in ops {
            match op {
                Op::RouteReplace(r) | Op::RouteAdd(r) => {
                    routes.retain(|e: &Route| e.dest != r.dest);
                    routes.push(r.clone());
                }
                Op::RouteDel(r) => routes.retain(|e: &Route| e.dest != r.dest),
                Op::NeighSet(n) => {
                    // AF_INET neigh on the vxlan dev = ARP (gw IP -> MAC);
                    // AF_BRIDGE neigh = FDB (MAC -> underlay endpoint).
                    if n.family == cave_home_cni_flannel::netlink::AF_BRIDGE {
                        fdb.insert(n.mac, n.ip);
                    } else {
                        arp.insert(n.ip, n.mac);
                    }
                }
                _ => {}
            }
        }
        Self {
            subnet,
            routes,
            arp,
            fdb,
        }
    }

    /// Longest-prefix match for `dst` in this node's route table.
    fn lookup_route(&self, dst: IpAddr) -> Option<&Route> {
        self.routes
            .iter()
            .filter(|r| r.dest.contains(dst))
            .max_by_key(|r| r.dest.prefix_len())
    }
}

/// The outcome of simulating one packet through the overlay.
#[derive(Debug, PartialEq, Eq)]
enum Delivery {
    /// Encapsulated and sent to this underlay endpoint, then decapsulated and
    /// delivered to a pod in the destination subnet.
    Encapsulated { underlay_endpoint: IpAddr },
    /// Delivered directly on the underlay via this gateway (host-gw).
    DirectRoute { gateway: IpAddr },
    /// No route — the packet is dropped.
    Dropped,
}

/// Simulate sending a packet from `src` to pod IP `dst` across the cluster.
/// Walks `src`'s programmed kernel state the same way Linux would.
fn deliver(src: &NodeKernel, dst: IpAddr) -> Delivery {
    let Some(route) = src.lookup_route(dst) else {
        return Delivery::Dropped;
    };
    match route.gw {
        // VXLAN route: gateway is the peer's overlay .0, on-link via the
        // vxlan device. Resolve gw -> VTEP MAC (ARP) -> underlay IP (FDB).
        Some(gw) if route.flags == cave_home_cni_flannel::netlink::RTNH_F_ONLINK => {
            let mac = src.arp.get(&gw).copied().expect("ARP entry for overlay gw");
            let endpoint = src.fdb.get(&mac).copied().expect("FDB entry for VTEP MAC");
            Delivery::Encapsulated {
                underlay_endpoint: endpoint,
            }
        }
        // Direct / host-gw route.
        Some(gw) => Delivery::DirectRoute { gateway: gw },
        None => Delivery::Dropped,
    }
}

/// Build a VXLAN node: lease nothing here (the lease is passed in), bring up the
/// device, configure the overlay address, then program the peer's lease.
fn build_vxlan_node(
    node: &str,
    subnet: Cidr,
    public_ip: IpAddr,
    vtep_mac: MacAddr,
    peer: &PeerLease,
) -> NodeKernel {
    let mut dp = MockDatapath::new();
    let dev = VxlanDevice::ensure(
        &mut dp,
        &VxlanDeviceAttrs {
            vni: 1,
            name: "flannel.1".to_owned(),
            underlay_mtu: 1500,
            vtep_index: 2,
            vtep_addr: Some(public_ip),
            vtep_port: 8472,
            gbp: false,
            learning: false,
            hw_addr: vtep_mac,
        },
    )
    .expect("ensure device");
    // flannel gives flannel.1 the subnet's .0 at the subnet prefix length.
    dev.configure(&mut dp, subnet.network(), subnet.prefix_len())
        .expect("configure");

    let nw = VxlanNetwork {
        dev,
        backend: VxlanConfig::default(),
        local_node: node.to_owned(),
        local_underlay: None,
    };
    nw.handle_event(&mut dp, &LeaseEvent::Added(peer.clone()))
        .expect("program peer");

    NodeKernel::from_ops(subnet, &dp.ops)
}

#[test]
fn vxlan_cross_node_pod_traffic_is_encapsulated_to_the_right_peer() {
    // Cluster pod CIDR 10.42.0.0/16, /24 per node — flannel defaults.
    let mut mgr = SubnetManager::new(cidr("10.42.0.0/16"), 24).expect("mgr");
    let lease_a = mgr.allocate("node-a").expect("lease a");
    let lease_b = mgr.allocate("node-b").expect("lease b");
    assert_eq!(lease_a.subnet, cidr("10.42.0.0/24"));
    assert_eq!(lease_b.subnet, cidr("10.42.1.0/24"));

    let (ip_a, ip_b) = (v4("192.168.1.1"), v4("192.168.1.2"));
    let (mac_a, mac_b) = (MacAddr::new([0x0a, 0, 0, 0, 0, 1]), MacAddr::new([0x0a, 0, 0, 0, 0, 2]));

    let peer_b = PeerLease {
        node: "node-b".to_owned(),
        subnet: lease_b.subnet,
        data: NodeBackendData::Vxlan {
            public_ip: ip_b,
            vtep_mac: mac_b,
        },
    };
    let peer_a = PeerLease {
        node: "node-a".to_owned(),
        subnet: lease_a.subnet,
        data: NodeBackendData::Vxlan {
            public_ip: ip_a,
            vtep_mac: mac_a,
        },
    };

    let node_a = build_vxlan_node("node-a", lease_a.subnet, ip_a, mac_a, &peer_b);
    let node_b = build_vxlan_node("node-b", lease_b.subnet, ip_b, mac_b, &peer_a);

    // Assign real pod IPs out of each node's subnet via IPAM.
    let mut ipam_a = PodIpam::new(lease_a.subnet).expect("ipam a");
    let mut ipam_b = PodIpam::new(lease_b.subnet).expect("ipam b");
    let pod_a = ipam_a.allocate().expect("pod a"); // 10.42.0.2
    let pod_b = ipam_b.allocate().expect("pod b"); // 10.42.1.2
    assert!(node_a.subnet.contains(pod_a));
    assert!(node_b.subnet.contains(pod_b));

    // pod on A -> pod on B: A encapsulates to B's underlay endpoint.
    assert_eq!(
        deliver(&node_a, pod_b),
        Delivery::Encapsulated {
            underlay_endpoint: ip_b
        },
        "A must tunnel B-subnet traffic to B's public IP"
    );
    // and the frame decapsulates into B's own subnet (delivered locally).
    assert!(node_b.subnet.contains(pod_b));

    // pod on B -> pod on A: symmetric, encapsulated to A's endpoint.
    assert_eq!(
        deliver(&node_b, pod_a),
        Delivery::Encapsulated {
            underlay_endpoint: ip_a
        }
    );

    // Traffic to a pod IP outside every node subnet is dropped (no route).
    assert_eq!(deliver(&node_a, v4("10.42.9.9")), Delivery::Dropped);

    // A never installs a route to its own subnet — same-node pod traffic does
    // not hit the overlay at all.
    assert!(node_a.lookup_route(pod_a).is_none());
}

#[test]
fn vxlan_three_node_each_reaches_the_other_two() {
    let mut mgr = SubnetManager::new(cidr("10.42.0.0/16"), 24).expect("mgr");
    let leases: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|n| (n.to_string(), mgr.allocate(n).expect("lease").subnet))
        .collect();
    let pubip = |i: usize| v4(&format!("192.168.1.{}", i + 1));
    let mac = |i: usize| MacAddr::new([0x0a, 0, 0, 0, 0, i as u8 + 1]);

    let all_peers: Vec<PeerLease> = leases
        .iter()
        .enumerate()
        .map(|(i, (n, sn))| PeerLease {
            node: n.clone(),
            subnet: *sn,
            data: NodeBackendData::Vxlan {
                public_ip: pubip(i),
                vtep_mac: mac(i),
            },
        })
        .collect();

    // Build each node by programming every *other* node's lease.
    for (i, (node, subnet)) in leases.iter().enumerate() {
        let mut dp = MockDatapath::new();
        let dev = VxlanDevice::ensure(
            &mut dp,
            &VxlanDeviceAttrs {
                vni: 1,
                name: "flannel.1".to_owned(),
                underlay_mtu: 1500,
                vtep_index: 2,
                vtep_addr: Some(pubip(i)),
                vtep_port: 8472,
                gbp: false,
                learning: false,
                hw_addr: mac(i),
            },
        )
        .expect("ensure");
        dev.configure(&mut dp, subnet.network(), subnet.prefix_len())
            .expect("configure");
        let nw = VxlanNetwork {
            dev,
            backend: VxlanConfig::default(),
            local_node: node.clone(),
            local_underlay: None,
        };
        let events: Vec<LeaseEvent> = all_peers.iter().cloned().map(LeaseEvent::Added).collect();
        nw.handle_events(&mut dp, &events).expect("program peers");
        let kernel = NodeKernel::from_ops(*subnet, &dp.ops);

        // This node must reach each of the other two by encapsulating to that
        // peer's public IP, and have no route to itself.
        for (j, (_, other_sn)) in leases.iter().enumerate() {
            let target = other_sn.network(); // a pod address in the peer subnet
            if i == j {
                assert!(kernel.lookup_route(target).is_none(), "no self route");
            } else {
                assert_eq!(
                    deliver(&kernel, other_sn.nth_address(2).expect("pod")),
                    Delivery::Encapsulated {
                        underlay_endpoint: pubip(j)
                    },
                    "node {i} must tunnel to node {j}"
                );
            }
        }
    }
}

#[test]
fn hostgw_cross_node_pod_traffic_uses_direct_route() {
    let mut mgr = SubnetManager::new(cidr("10.42.0.0/16"), 24).expect("mgr");
    let sn_a = mgr.allocate("node-a").expect("a").subnet;
    let sn_b = mgr.allocate("node-b").expect("b").subnet;
    let (ip_a, ip_b) = (v4("192.168.1.1"), v4("192.168.1.2"));

    // node-a's host-gw backend programs a direct route to b's subnet via b's IP.
    let be_a = HostGwBackend::new(ExternalInterface {
        name: "eth0".to_owned(),
        index: 4,
        iface_addr: ip_a,
        ext_addr: ip_a,
        mtu: 1500,
    })
    .expect("backend a");
    let mut nw_a = be_a.register("node-a".to_owned());
    let mut dp_a = MockDatapath::new();
    nw_a
        .handle_event(
            &mut dp_a,
            &LeaseEvent::Added(PeerLease {
                node: "node-b".to_owned(),
                subnet: sn_b,
                data: NodeBackendData::HostGw { public_ip: ip_b },
            }),
        )
        .expect("program b");

    let node_a = NodeKernel::from_ops(sn_a, &dp_a.ops);
    // pod on A -> pod on B: directly routed via B's underlay IP, no encap.
    assert_eq!(
        deliver(&node_a, sn_b.nth_address(5).expect("pod b")),
        Delivery::DirectRoute { gateway: ip_b }
    );
    assert_eq!(deliver(&node_a, v4("10.42.7.7")), Delivery::Dropped);
}
