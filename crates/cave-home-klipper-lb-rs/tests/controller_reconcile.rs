// SPDX-License-Identifier: Apache-2.0
//! `ServiceLB` controller reconcile-loop tests (RED until `controller` lands).
//!
//! These exercise the decision core of K3s's `ServiceLB` controller
//! (`pkg/cloudprovider/servicelb.go` `updateStatus`/`deployDaemonSet`/
//! `deleteDaemonSet`): given a cluster snapshot of `LoadBalancer` Services +
//! Nodes + the set of svclb `DaemonSets` that currently exist, decide which
//! `DaemonSets` to apply (create-or-update), which to delete (orphans), and what
//! each Service's `status.loadBalancer.ingress` should become — composing the
//! already-tested port allocator, node selector, pod-spec builder, and status
//! computer.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use cave_home_klipper_lb_rs::controller::{ReconcileContext, ServiceDisposition, reconcile};
use cave_home_klipper_lb_rs::daemonset::SvclbPodSpec;
use cave_home_klipper_lb_rs::node::Node;
use cave_home_klipper_lb_rs::service::{
    ExternalTrafficPolicy, LoadBalancerService, Protocol, ServicePort,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ip(s: &str) -> Result<IpAddr, std::net::AddrParseError> {
    s.parse()
}

fn port(name: &str, proto: Protocol, p: u16, np: u16) -> ServicePort {
    ServicePort {
        name: name.to_owned(),
        protocol: proto,
        port: p,
        node_port: np,
    }
}

fn svc(name: &str, ports: Vec<ServicePort>) -> LoadBalancerService {
    LoadBalancerService {
        namespace: "default".to_owned(),
        name: name.to_owned(),
        load_balancer_ips: vec![],
        ports,
        external_traffic_policy: ExternalTrafficPolicy::Cluster,
        node_selector: BTreeMap::new(),
    }
}

fn http(name: &str) -> LoadBalancerService {
    svc(name, vec![port("http", Protocol::Tcp, 80, 30080)])
}

#[test]
fn empty_cluster_reconciles_to_nothing() {
    let r = reconcile(&[], &[], &ReconcileContext::default());
    assert_eq!(r.apply, [] as [SvclbPodSpec; 0]);
    assert_eq!(r.delete, [] as [String; 0]);
    assert_eq!(r.dispositions, [] as [(String, ServiceDisposition); 0]);
}

#[test]
fn lone_loadbalancer_service_is_programmed_and_applied() -> TestResult {
    let services = vec![http("web")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];

    let r = reconcile(&services, &nodes, &ReconcileContext::default());

    // One DaemonSet to apply, named svclb-web.
    assert_eq!(r.apply.len(), 1);
    assert_eq!(r.apply[0].daemonset_name, "svclb-web");
    assert_eq!(r.delete, [] as [String; 0]);

    // The Service is programmed, publishing the node's internal IP.
    let d = r.disposition("default/web").ok_or("disposition present")?;
    assert!(d.is_programmed());
    assert_eq!(d.ingress_ips(), &[ip("10.0.0.1")?]);
    Ok(())
}

#[test]
fn programmed_service_carries_its_daemonset_name() -> TestResult {
    let services = vec![http("web")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let r = reconcile(&services, &nodes, &ReconcileContext::default());
    match r.disposition("default/web").ok_or("disposition recorded")? {
        ServiceDisposition::Programmed { daemonset, .. } => assert_eq!(daemonset, "svclb-web"),
        other => return Err(format!("expected Programmed, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn orphan_daemonset_is_deleted() {
    // No services, but a stale svclb DaemonSet still exists -> delete it.
    let ctx = ReconcileContext::default()
        .with_existing_daemonsets(["svclb-gone".to_owned(), "svclb-old".to_owned()]);
    let r = reconcile(&[], &[], &ctx);
    assert_eq!(r.apply, [] as [SvclbPodSpec; 0]);
    assert_eq!(
        r.delete,
        vec!["svclb-gone".to_owned(), "svclb-old".to_owned()]
    );
}

#[test]
fn existing_daemonset_for_live_service_is_not_deleted() -> TestResult {
    // svclb-web already exists AND the web Service still exists -> keep (apply),
    // never delete.
    let services = vec![http("web")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let ctx = ReconcileContext::default().with_existing_daemonsets(["svclb-web".to_owned()]);
    let r = reconcile(&services, &nodes, &ctx);
    assert_eq!(r.apply.len(), 1);
    assert!(
        r.delete.is_empty(),
        "live service's DaemonSet must not be deleted"
    );
    Ok(())
}

#[test]
fn mixed_reconcile_applies_live_and_deletes_orphan() -> TestResult {
    let services = vec![http("web")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let ctx = ReconcileContext::default()
        .with_existing_daemonsets(["svclb-web".to_owned(), "svclb-dead".to_owned()]);
    let r = reconcile(&services, &nodes, &ctx);
    assert_eq!(r.apply.len(), 1);
    assert_eq!(r.apply[0].daemonset_name, "svclb-web");
    assert_eq!(r.delete, vec!["svclb-dead".to_owned()]);
    Ok(())
}

#[test]
fn second_service_on_same_host_port_is_left_pending() -> TestResult {
    // web takes TCP/80 first; blog wants TCP/80 too -> blog is pending, gets no
    // DaemonSet (matches K3s leaving the second LB Service <pending>).
    let services = vec![http("web"), http("blog")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let r = reconcile(&services, &nodes, &ReconcileContext::default());

    assert_eq!(r.apply.len(), 1, "only the winner gets a DaemonSet");
    assert_eq!(r.apply[0].daemonset_name, "svclb-web");

    assert!(
        r.disposition("default/web")
            .ok_or("disposition recorded")?
            .is_programmed()
    );
    match r
        .disposition("default/blog")
        .ok_or("disposition recorded")?
    {
        ServiceDisposition::Pending { conflicts } => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].held_by, "default/web");
            assert_eq!(conflicts[0].requested_by, "default/blog");
        }
        other => return Err(format!("expected Pending, got {other:?}").into()),
    }
    // A pending service publishes no ingress IPs.
    assert_eq!(
        r.disposition("default/blog")
            .ok_or("disposition recorded")?
            .ingress_ips(),
        [] as [IpAddr; 0]
    );
    Ok(())
}

#[test]
fn structurally_invalid_service_is_skipped_not_panicked() -> TestResult {
    // nodePort 0 is invalid; the controller skips it (logs) and programs nothing.
    let bad = svc("broken", vec![port("http", Protocol::Tcp, 80, 0)]);
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let r = reconcile(&[bad], &nodes, &ReconcileContext::default());
    assert_eq!(r.apply, [] as [SvclbPodSpec; 0]);
    match r
        .disposition("default/broken")
        .ok_or("disposition recorded")?
    {
        ServiceDisposition::Invalid { .. } => {}
        other => return Err(format!("expected Invalid, got {other:?}").into()),
    }
    // An invalid service does not consume the host port: a later valid service
    // on the same port is still programmable.
    let good = http("ok");
    let r2 = reconcile(
        &[
            svc("broken", vec![port("http", Protocol::Tcp, 80, 0)]),
            good,
        ],
        &nodes,
        &ReconcileContext::default(),
    );
    assert_eq!(r2.apply.len(), 1);
    assert_eq!(r2.apply[0].daemonset_name, "svclb-ok");
    Ok(())
}

#[test]
fn dest_ips_flow_into_the_pod_spec_env() -> TestResult {
    // The cluster Service ClusterIP supplied for DEST_IPS reaches the container env.
    let services = vec![http("web")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let mut dest = BTreeMap::new();
    dest.insert("default/web".to_owned(), vec![ip("10.43.0.7")?]);
    let ctx = ReconcileContext::default().with_dest_ips(dest);

    let r = reconcile(&services, &nodes, &ctx);
    let spec = &r.apply[0];
    assert_eq!(spec.containers[0].env_value("DEST_IPS"), Some("10.43.0.7"));
    assert_eq!(spec.containers[0].env_value("DEST_PORT"), Some("30080"));
    Ok(())
}

#[test]
fn local_etp_only_publishes_nodes_with_a_backing_pod() -> TestResult {
    let mut s = http("web");
    s.external_traffic_policy = ExternalTrafficPolicy::Local;
    let nodes = vec![
        Node::new("n1").with_internal_ip(ip("10.0.0.1")?),
        Node::new("n2").with_internal_ip(ip("10.0.0.2")?),
    ];
    let mut backing = BTreeMap::new();
    backing.insert("default/web".to_owned(), BTreeSet::from(["n2".to_owned()]));
    let ctx = ReconcileContext::default().with_backing_pods(backing);

    let r = reconcile(&[s], &nodes, &ctx);
    let d = r.disposition("default/web").ok_or("disposition recorded")?;
    assert!(
        d.is_programmed(),
        "DaemonSet still deployed under Local ETP"
    );
    assert_eq!(d.ingress_ips(), &[ip("10.0.0.2")?]);
    Ok(())
}

#[test]
fn status_patches_lists_every_programmed_service_with_its_ingress() -> TestResult {
    let services = [http("web"), http("api2")];
    // api2 uses a different port so both program.
    let mut api = svc("api2", vec![port("h", Protocol::Tcp, 8080, 30090)]);
    api.name = "api2".to_owned();
    let services = vec![services[0].clone(), api];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];

    let r = reconcile(&services, &nodes, &ReconcileContext::default());
    let patches = r.status_patches();
    assert_eq!(patches.len(), 2);
    let by_key: BTreeMap<_, _> = patches.into_iter().collect();
    assert_eq!(by_key.get("default/web"), Some(&vec![ip("10.0.0.1")?]));
    assert_eq!(by_key.get("default/api2"), Some(&vec![ip("10.0.0.1")?]));
    Ok(())
}

#[test]
fn reconcile_is_deterministic_across_repeated_calls() -> TestResult {
    let services = vec![http("web"), http("blog")];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1")?)];
    let a = reconcile(&services, &nodes, &ReconcileContext::default());
    let b = reconcile(&services, &nodes, &ReconcileContext::default());
    assert_eq!(a.apply, b.apply);
    assert_eq!(a.delete, b.delete);
    Ok(())
}
