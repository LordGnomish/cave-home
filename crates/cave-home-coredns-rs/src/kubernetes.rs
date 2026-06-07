// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `kubernetes` plugin: cluster service discovery.
//!
//! Resolves the `CoreDNS` Kubernetes name schema under a cluster zone (default
//! `cluster.local`):
//!
//! * `service.namespace.svc.zone` — `A`/`AAAA` to the `ClusterIP`, or to every
//!   endpoint address for a headless service; `CNAME` for an `ExternalName`.
//! * `endpoint.service.namespace.svc.zone` — a single headless endpoint.
//! * `_port._proto.service.namespace.svc.zone` — `SRV` for a named port, with
//!   the target's address offered in the additional section.
//! * `a-b-c-d.namespace.pod.zone` — the pod address synthesised from the name
//!   (the `pods insecure` mode).
//! * reverse `*.in-addr.arpa` / `*.ip6.arpa` — `PTR` to the owning service FQDN.
//!
//! An unknown service in a known namespace, or an unknown namespace, is
//! authoritative `NXDOMAIN`; an existing name queried for a type it has no
//! record of is `NODATA`. Names outside the plugin's zones are deferred to the
//! rest of the chain; an in-zone miss may also be deferred with `fallthrough`.
//!
//! The data here is an in-memory snapshot supplied by the caller. The live
//! Kubernetes API watch that would populate it is deferred (`parity.manifest`).

use crate::arpa::from_arpa;
use crate::name::Name;
use crate::plugin::{Next, Outcome, Plugin, Request};
use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
use crate::wire::Rcode;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

/// The default record TTL for cluster answers (`CoreDNS` default).
const DEFAULT_TTL: u32 = 5;

/// A service port (used for `SRV`).
#[derive(Debug, Clone)]
pub struct Port {
    /// The port name (the `_port` label, without the underscore).
    pub name: String,
    /// The transport protocol.
    pub protocol: Protocol,
    /// The port number.
    pub number: u16,
}

impl Port {
    /// A TCP port.
    #[must_use]
    pub fn tcp(name: &str, number: u16) -> Self {
        Self { name: name.to_string(), protocol: Protocol::Tcp, number }
    }

    /// A UDP port.
    #[must_use]
    pub fn udp(name: &str, number: u16) -> Self {
        Self { name: name.to_string(), protocol: Protocol::Udp, number }
    }
}

/// A transport protocol, as it appears in the `_proto` SRV label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `_tcp`.
    Tcp,
    /// `_udp`.
    Udp,
}

impl Protocol {
    /// The SRV label form (`_tcp` / `_udp`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "_tcp",
            Self::Udp => "_udp",
        }
    }
}

/// A single endpoint behind a headless service.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// The optional endpoint hostname (resolvable as a sub-name of the service).
    pub hostname: Option<String>,
    /// The endpoint address.
    pub ip: IpAddr,
}

impl Endpoint {
    /// Build an endpoint.
    #[must_use]
    pub fn new(hostname: Option<&str>, ip: IpAddr) -> Self {
        Self { hostname: hostname.map(str::to_string), ip }
    }
}

/// What a service resolves to.
#[derive(Debug, Clone)]
enum Spec {
    /// A normal service with one or more cluster addresses.
    ClusterIp(Vec<IpAddr>),
    /// A headless service: addresses come from the endpoints.
    Headless(Vec<Endpoint>),
    /// An alias to an external name.
    ExternalName(Name),
}

/// A Kubernetes service.
#[derive(Debug, Clone)]
pub struct Service {
    namespace: String,
    name: String,
    spec: Spec,
    ports: Vec<Port>,
}

impl Service {
    /// A normal service exposing one or more `ClusterIP` addresses.
    #[must_use]
    pub fn cluster_ip(namespace: &str, name: &str, ips: Vec<IpAddr>) -> Self {
        Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            spec: Spec::ClusterIp(ips),
            ports: Vec::new(),
        }
    }

    /// A headless service backed by the given endpoints.
    #[must_use]
    pub fn headless(namespace: &str, name: &str, endpoints: Vec<Endpoint>) -> Self {
        Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            spec: Spec::Headless(endpoints),
            ports: Vec::new(),
        }
    }

    /// An `ExternalName` alias service.
    #[must_use]
    pub fn external_name(namespace: &str, name: &str, target: Name) -> Self {
        Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            spec: Spec::ExternalName(target),
            ports: Vec::new(),
        }
    }

    /// Add a named port (builder style).
    #[must_use]
    pub fn with_port(mut self, port: Port) -> Self {
        self.ports.push(port);
        self
    }

    /// All addresses this service currently resolves to.
    fn addresses(&self) -> Vec<IpAddr> {
        match &self.spec {
            Spec::ClusterIp(ips) => ips.clone(),
            Spec::Headless(eps) => eps.iter().map(|e| e.ip).collect(),
            Spec::ExternalName(_) => Vec::new(),
        }
    }
}

/// The `kubernetes` plugin: an in-memory service registry.
pub struct Kubernetes {
    zones: Vec<Name>,
    services: HashMap<(String, String), Service>,
    namespaces: HashSet<String>,
    reverse: HashMap<IpAddr, Name>,
    ttl: u32,
    fallthrough: bool,
    pods_insecure: bool,
}

impl Kubernetes {
    /// A plugin authoritative for `zone` (e.g. `cluster.local`).
    #[must_use]
    pub fn new(zone: &str) -> Self {
        let zones = Name::parse(zone).map_or_else(|_| vec![Name::root()], |z| vec![z]);
        Self {
            zones,
            services: HashMap::new(),
            namespaces: HashSet::new(),
            reverse: HashMap::new(),
            ttl: DEFAULT_TTL,
            fallthrough: false,
            pods_insecure: true,
        }
    }

    /// Register a namespace (so an empty namespace is distinguished from a
    /// non-existent one).
    #[must_use]
    pub fn with_namespace(mut self, ns: &str) -> Self {
        self.namespaces.insert(ns.to_string());
        self
    }

    /// Register a service, indexing it for forward and reverse lookups.
    #[must_use]
    pub fn with_service(mut self, svc: Service) -> Self {
        self.namespaces.insert(svc.namespace.clone());
        if let Some(zone) = self.zones.first() {
            let fqdn = service_fqdn(&svc.namespace, &svc.name, zone);
            for ip in svc.addresses() {
                self.reverse.entry(ip).or_insert_with(|| fqdn.clone());
            }
        }
        self.services.insert((svc.namespace.clone(), svc.name.clone()), svc);
        self
    }

    /// Override the answer TTL.
    #[must_use]
    pub const fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl;
        self
    }

    /// Enable `fallthrough` for in-zone misses.
    #[must_use]
    pub const fn with_fallthrough(mut self, on: bool) -> Self {
        self.fallthrough = on;
        self
    }

    /// Toggle insecure pod synthesis (default on).
    #[must_use]
    pub const fn with_pods_insecure(mut self, on: bool) -> Self {
        self.pods_insecure = on;
        self
    }

    /// The labels of `name` with the matching zone stripped, or `None` if the
    /// name is not under any of the plugin's zones.
    fn strip_zone(&self, name: &Name) -> Option<Vec<Vec<u8>>> {
        let zone = self.zones.iter().find(|z| name.is_subdomain_of(z))?;
        let keep = name.label_count() - zone.label_count();
        Some(name.labels()[..keep].to_vec())
    }

    const fn record(&self, owner: Name, rdata: Rdata) -> ResourceRecord {
        ResourceRecord::new(owner, Class::In, self.ttl, rdata)
    }

    /// The authoritative reply for an in-zone request, or the
    /// fallthrough/NODATA/NXDOMAIN decision.
    fn finish(&self, req: &Request<'_>, next: Next<'_>, answers: Vec<ResourceRecord>, found: Found) -> Outcome {
        match found {
            Found::Answers if !answers.is_empty() => {
                let mut reply = req.reply().with_aa(true);
                reply.answers = answers;
                Ok(reply)
            }
            // Name exists, wrong type → NODATA.
            Found::Answers | Found::NoData => Ok(req.reply().with_aa(true)),
            Found::Nxdomain => {
                if self.fallthrough {
                    next.run(req)
                } else {
                    Ok(req.reply().with_aa(true).with_rcode(Rcode::NxDomain))
                }
            }
        }
    }
}

/// The disposition of an in-zone lookup.
#[derive(Clone, Copy)]
enum Found {
    /// Records were produced (or, if empty, treat as NXDOMAIN below).
    Answers,
    /// The name exists but has no record of the queried type.
    NoData,
    /// The name does not exist.
    Nxdomain,
}

/// The fully-qualified name of a service: `name.namespace.svc.zone`.
fn service_fqdn(namespace: &str, name: &str, zone: &Name) -> Name {
    let mut labels = vec![name.as_bytes().to_vec(), namespace.as_bytes().to_vec(), b"svc".to_vec()];
    labels.extend(zone.labels().iter().cloned());
    Name::from_labels(labels).unwrap_or_else(|_| zone.clone())
}

/// Parse a dashed pod-name IP (`1-2-3-4`, or v6 with dashes).
fn parse_dashed_ip(label: &[u8]) -> Option<IpAddr> {
    let s = core::str::from_utf8(label).ok()?;
    if let Ok(v4) = s.replace('-', ".").parse() {
        return Some(IpAddr::V4(v4));
    }
    s.replace('-', ":").parse().ok().map(IpAddr::V6)
}

/// Whether `ip` matches the address family the query type asks for.
const fn family_matches(qtype: RecordType, ip: IpAddr) -> bool {
    matches!(
        (qtype, ip),
        (RecordType::A, IpAddr::V4(_)) | (RecordType::Aaaa, IpAddr::V6(_))
    )
}

/// Build an address record of the right type for `ip`.
const fn addr_rdata(ip: IpAddr) -> Rdata {
    match ip {
        IpAddr::V4(a) => Rdata::A(a),
        IpAddr::V6(a) => Rdata::Aaaa(a),
    }
}

impl Kubernetes {
    /// Resolve a `service.namespace` A/AAAA/CNAME query.
    fn resolve_service(
        &self,
        owner: &Name,
        service: &str,
        namespace: &str,
        qtype: RecordType,
    ) -> (Vec<ResourceRecord>, Found) {
        let Some(svc) = self.services.get(&(namespace.to_string(), service.to_string())) else {
            // Both an unknown service in a known namespace and an unknown
            // namespace are NXDOMAIN for a non-wildcard query.
            return (Vec::new(), Found::Nxdomain);
        };
        match &svc.spec {
            Spec::ExternalName(target) => {
                (vec![self.record(owner.clone(), Rdata::Cname(target.clone()))], Found::Answers)
            }
            Spec::ClusterIp(_) | Spec::Headless(_) => {
                let answers: Vec<_> = svc
                    .addresses()
                    .into_iter()
                    .filter(|ip| family_matches(qtype, *ip))
                    .map(|ip| self.record(owner.clone(), addr_rdata(ip)))
                    .collect();
                if answers.is_empty() {
                    (answers, Found::NoData) // service exists, wrong family
                } else {
                    (answers, Found::Answers)
                }
            }
        }
    }

    /// Resolve a single headless endpoint by hostname.
    fn resolve_endpoint(
        &self,
        owner: &Name,
        endpoint: &str,
        service: &str,
        namespace: &str,
        qtype: RecordType,
    ) -> (Vec<ResourceRecord>, Found) {
        let Some(svc) = self.services.get(&(namespace.to_string(), service.to_string())) else {
            return (Vec::new(), Found::Nxdomain);
        };
        let Spec::Headless(eps) = &svc.spec else {
            return (Vec::new(), Found::Nxdomain);
        };
        let Some(ep) = eps.iter().find(|e| e.hostname.as_deref() == Some(endpoint)) else {
            return (Vec::new(), Found::Nxdomain);
        };
        if family_matches(qtype, ep.ip) {
            (vec![self.record(owner.clone(), addr_rdata(ep.ip))], Found::Answers)
        } else {
            (Vec::new(), Found::NoData)
        }
    }

    /// Resolve an `_port._proto.service.namespace` SRV query.
    fn resolve_srv(
        &self,
        owner: &Name,
        port_label: &str,
        proto_label: &str,
        service: &str,
        namespace: &str,
    ) -> (Vec<ResourceRecord>, Vec<ResourceRecord>, Found) {
        let Some(svc) = self.services.get(&(namespace.to_string(), service.to_string())) else {
            return (Vec::new(), Vec::new(), Found::Nxdomain);
        };
        let want = port_label.strip_prefix('_').unwrap_or(port_label);
        let Some(port) = svc
            .ports
            .iter()
            .find(|p| p.name == want && p.protocol.label() == proto_label)
        else {
            return (Vec::new(), Vec::new(), Found::NoData);
        };
        let Some(zone) = self.zones.first() else {
            return (Vec::new(), Vec::new(), Found::Nxdomain);
        };
        let target = service_fqdn(namespace, service, zone);
        let srv = self.record(
            owner.clone(),
            Rdata::Srv { priority: 0, weight: 100, port: port.number, target: target.clone() },
        );
        // Offer the target's addresses in the additional section.
        let additional: Vec<_> = svc
            .addresses()
            .into_iter()
            .map(|ip| self.record(target.clone(), addr_rdata(ip)))
            .collect();
        (vec![srv], additional, Found::Answers)
    }

    /// Resolve an `a-b-c-d.namespace.pod` query.
    fn resolve_pod(
        &self,
        owner: &Name,
        ip_label: &[u8],
        namespace: &str,
        qtype: RecordType,
    ) -> (Vec<ResourceRecord>, Found) {
        if !self.pods_insecure {
            return (Vec::new(), Found::Nxdomain);
        }
        let _ = namespace;
        match parse_dashed_ip(ip_label) {
            Some(ip) if family_matches(qtype, ip) => {
                (vec![self.record(owner.clone(), addr_rdata(ip))], Found::Answers)
            }
            Some(_) => (Vec::new(), Found::NoData),
            None => (Vec::new(), Found::Nxdomain),
        }
    }
}

impl Plugin for Kubernetes {
    fn name(&self) -> &'static str {
        "kubernetes"
    }

    #[allow(clippy::too_many_lines)]
    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        let Some(q) = req.question() else { return next.run(req) };
        let owner = q.name.clone();

        // Reverse lookups are matched against the address index regardless of
        // the plugin's forward zones.
        if q.qtype == RecordType::Ptr {
            if let Some(ip) = from_arpa(&owner) {
                if let Some(fqdn) = self.reverse.get(&ip) {
                    let mut reply = req.reply().with_aa(true);
                    reply.answers.push(self.record(owner, Rdata::Ptr(fqdn.clone())));
                    return Ok(reply);
                }
            }
            return next.run(req);
        }

        // Forward queries must be inside one of the plugin's zones.
        let Some(labels) = self.strip_zone(&owner) else {
            return next.run(req);
        };
        let Some((kind, middle)) = labels.split_last() else {
            return next.run(req);
        };
        let middle: Vec<String> =
            middle.iter().map(|l| String::from_utf8_lossy(l).into_owned()).collect();

        let (answers, additional, found) = match (kind.as_slice(), middle.as_slice()) {
            // _port._proto.service.namespace.svc
            (b"svc", [port, proto, service, namespace]) if q.qtype == RecordType::Srv => {
                let (a, add, f) = self.resolve_srv(&owner, port, proto, service, namespace);
                (a, add, f)
            }
            // endpoint.service.namespace.svc
            (b"svc", [endpoint, service, namespace]) => {
                let (a, f) = self.resolve_endpoint(&owner, endpoint, service, namespace, q.qtype);
                (a, Vec::new(), f)
            }
            // service.namespace.svc
            (b"svc", [service, namespace]) => {
                let (a, f) = self.resolve_service(&owner, service, namespace, q.qtype);
                (a, Vec::new(), f)
            }
            // a-b-c-d.namespace.pod
            (b"pod", [ip_label, namespace]) => {
                let (a, f) = self.resolve_pod(&owner, ip_label.as_bytes(), namespace, q.qtype);
                (a, Vec::new(), f)
            }
            _ => (Vec::new(), Vec::new(), Found::Nxdomain),
        };

        let outcome = self.finish(req, next, answers, found)?;
        // Attach the SRV additional section if we produced one and answered.
        if !additional.is_empty() && outcome.header.rcode == Rcode::NoError && !outcome.answers.is_empty() {
            let mut outcome = outcome;
            outcome.additional = additional;
            return Ok(outcome);
        }
        Ok(outcome)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::arpa::to_arpa;
    use crate::message::Message;
    use crate::name::Name;
    use crate::plugin::{Chain, Next, Outcome, Plugin, Request};
    use crate::rr::{Rdata, RecordType};
    use crate::wire::Rcode;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    struct Sentinel;
    impl Plugin for Sentinel {
        fn name(&self) -> &'static str {
            "sentinel"
        }
        fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
            Ok(req.reply().with_rcode(Rcode::Refused))
        }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn k8s() -> Kubernetes {
        Kubernetes::new("cluster.local")
            .with_namespace("default")
            .with_namespace("kube-system")
            .with_service(
                Service::cluster_ip("default", "web", vec![ip("10.0.0.1")])
                    .with_port(Port::tcp("http", 80)),
            )
            .with_service(Service::cluster_ip("default", "db6", vec![ip("fd00::1")]))
            .with_service(Service::headless(
                "default",
                "cache",
                vec![
                    Endpoint::new(Some("c0"), ip("10.0.1.1")),
                    Endpoint::new(Some("c1"), ip("10.0.1.2")),
                ],
            ))
            .with_service(Service::external_name(
                "default",
                "ext",
                Name::parse("example.com").unwrap(),
            ))
    }

    fn ask(name: &str, t: RecordType) -> Message {
        let chain = Chain::new(vec![Box::new(k8s()), Box::new(Sentinel)]);
        chain.handle(&Message::query(Name::parse(name).unwrap(), t, 1))
    }

    #[test]
    fn cluster_ip_a_record() {
        let m = ask("web.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert!(m.header.aa);
        assert_eq!(m.answers.len(), 1);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn cluster_ip_aaaa_record() {
        let m = ask("db6.default.svc.cluster.local", RecordType::Aaaa);
        assert_eq!(
            m.answers[0].rdata,
            Rdata::Aaaa(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1))
        );
    }

    #[test]
    fn headless_returns_all_endpoint_addresses() {
        let m = ask("cache.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.answers.len(), 2);
        let ips: Vec<_> = m
            .answers
            .iter()
            .filter_map(|rr| match rr.rdata {
                Rdata::A(a) => Some(a),
                _ => None,
            })
            .collect();
        assert!(ips.contains(&Ipv4Addr::new(10, 0, 1, 1)));
        assert!(ips.contains(&Ipv4Addr::new(10, 0, 1, 2)));
    }

    #[test]
    fn headless_endpoint_hostname_resolves() {
        let m = ask("c0.cache.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.answers.len(), 1);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 1, 1)));
    }

    #[test]
    fn external_name_returns_cname() {
        let m = ask("ext.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.answers.len(), 1);
        assert_eq!(m.answers[0].rdata, Rdata::Cname(Name::parse("example.com").unwrap()));
    }

    #[test]
    fn srv_for_named_port() {
        let m = ask("_http._tcp.web.default.svc.cluster.local", RecordType::Srv);
        assert_eq!(m.header.rcode, Rcode::NoError);
        let srv = m.answers.iter().find_map(|rr| match &rr.rdata {
            Rdata::Srv { port, target, .. } => Some((*port, target.clone())),
            _ => None,
        });
        let (port, target) = srv.expect("an SRV answer");
        assert_eq!(port, 80);
        assert_eq!(target, Name::parse("web.default.svc.cluster.local").unwrap());
        // The target's A record is offered in the additional section.
        assert!(m.additional.iter().any(|rr| matches!(rr.rdata, Rdata::A(_))));
    }

    #[test]
    fn pod_insecure_synthesizes_from_the_name() {
        let m = ask("1-2-3-4.default.pod.cluster.local", RecordType::A);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(1, 2, 3, 4)));
    }

    #[test]
    fn unknown_service_in_known_namespace_is_nxdomain() {
        let m = ask("nope.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NxDomain);
        assert!(m.header.aa);
    }

    #[test]
    fn unknown_namespace_is_nxdomain() {
        let m = ask("web.ghost.svc.cluster.local", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NxDomain);
    }

    #[test]
    fn wrong_family_for_existing_service_is_nodata() {
        // web has only an IPv4 ClusterIP; AAAA is NODATA, not NXDOMAIN.
        let m = ask("web.default.svc.cluster.local", RecordType::Aaaa);
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert!(m.answers.is_empty());
    }

    #[test]
    fn reverse_ptr_resolves_cluster_ip_to_service_fqdn() {
        let rev = to_arpa(ip("10.0.0.1"));
        let chain = Chain::new(vec![Box::new(k8s()), Box::new(Sentinel)]);
        let m = chain.handle(&Message::query(rev, RecordType::Ptr, 1));
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert_eq!(
            m.answers[0].rdata,
            Rdata::Ptr(Name::parse("web.default.svc.cluster.local").unwrap())
        );
    }

    #[test]
    fn out_of_zone_names_are_deferred_downstream() {
        // example.org is not in cluster.local; kubernetes must defer to the
        // sentinel rather than answering.
        let m = ask("www.example.org", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::Refused);
    }

    #[test]
    fn in_zone_miss_falls_through_when_configured() {
        let chain = Chain::new(vec![
            Box::new(k8s().with_fallthrough(true)),
            Box::new(Sentinel),
        ]);
        let m = chain.handle(&Message::query(
            Name::parse("nope.default.svc.cluster.local").unwrap(),
            RecordType::A,
            1,
        ));
        assert_eq!(m.header.rcode, Rcode::Refused);
    }
}
