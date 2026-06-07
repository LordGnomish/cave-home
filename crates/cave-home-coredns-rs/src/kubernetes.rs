// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `kubernetes` plugin: cluster service discovery.

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
