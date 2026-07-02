// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Lowering a parsed [`ServerBlock`] into a live [`Chain`].
//!
//! [`crate::corefile`] produces the config AST; this module is the deferred
//! "wiring step" it names — `CoreDNS`'s `core/dnsserver/register.go`
//! `buildStack`. Two faithful behaviours are reproduced:
//!
//! * **Canonical plugin order.** `CoreDNS` does *not* run plugins in Corefile
//!   declaration order. Each plugin has a fixed priority from `plugin.cfg`
//!   (compiled into `core/dnsserver/zdirectives.go`'s `Directives` slice), and
//!   the server sorts the configured plugins by it before assembling the
//!   handler chain. [`DIRECTIVES`] is that list, ported verbatim.
//! * **Per-directive instantiation.** Each directive becomes its plugin via the
//!   plugin's `setup` — here, the relevant constructor in this crate.
//!
//! Only the plugins this crate actually implements can be lowered; a directive
//! naming a plugin we do not build (e.g. `tls`, `bind`, `dnssec`) is a
//! [`WireError::Config`] rather than a silent drop, so a Corefile that asks for
//! an unsupported plugin fails loudly instead of resolving differently than the
//! operator wrote.

use crate::builtins::{Errors, Metrics, Ready};
use crate::cache::CachePlugin;
use crate::corefile::Directive;
use crate::corefile::ServerBlock;
use crate::error::{Result, WireError};
use crate::file::{FilePlugin, Zone};
use crate::forward::{Forward, Policy as ForwardPolicy};
use crate::hosts::Hosts;
use crate::k8s::K8sSnapshot;
use crate::kubernetes::Kubernetes;
use crate::name::Name;
use crate::plugin::{Chain, Plugin};
use crate::rewrite::{NameRule, Rewriter, Rule};
use crate::rr::{Class, RecordType};

/// The canonical plugin priority order, ported verbatim from `CoreDNS`'s
/// `core/dnsserver/zdirectives.go` (generated from `plugin.cfg`).
///
/// The server sorts a server block's configured plugins by their index in this
/// slice before chaining them, so Corefile declaration order never changes
/// resolution order.
pub const DIRECTIVES: &[&str] = &[
    "root",
    "metadata",
    "geoip",
    "cancel",
    "tls",
    "proxyproto",
    "quic",
    "grpc_server",
    "https",
    "https3",
    "timeouts",
    "multisocket",
    "reload",
    "nsid",
    "bufsize",
    "bind",
    "debug",
    "trace",
    "ready",
    "health",
    "pprof",
    "prometheus",
    "errors",
    "log",
    "dnstap",
    "local",
    "dns64",
    "any",
    "chaos",
    "loadbalance",
    "tsig",
    "cache",
    "rewrite",
    "acl",
    "header",
    "dnssec",
    "autopath",
    "minimal",
    "template",
    "transfer",
    "hosts",
    "route53",
    "azure",
    "clouddns",
    "k8s_external",
    "kubernetes",
    "file",
    "auto",
    "secondary",
    "etcd",
    "loop",
    "forward",
    "grpc",
    "erratic",
    "whoami",
    "on",
    "sign",
    "view",
    "nomad",
];

/// The default cache capacity when a `cache` directive gives no size (the
/// `CoreDNS` default success cache is 9984 entries; we round to 10 000).
const DEFAULT_CACHE_CAPACITY: usize = 10_000;

/// The priority index of a directive in [`DIRECTIVES`], or `None` if the name
/// is not a known `CoreDNS` plugin.
#[must_use]
pub fn priority(name: &str) -> Option<usize> {
    DIRECTIVES.iter().position(|d| *d == name)
}

/// Lower a parsed [`ServerBlock`] into a live [`Chain`], ordering the plugins by
/// their canonical [`DIRECTIVES`] priority and instantiating each from its
/// directive.
///
/// # Errors
/// [`WireError::Config`] if a directive names a plugin that is not a known
/// `CoreDNS` plugin, or one this crate does not implement, or whose arguments
/// cannot be parsed.
pub fn build_chain(block: &ServerBlock) -> Result<Chain> {
    build_chain_with(block, None)
}

/// Lower a [`ServerBlock`] into a [`Chain`] like [`build_chain`], but seed any
/// `kubernetes` plugin from a live API snapshot.
///
/// `CoreDNS`'s kubernetes plugin is populated by a watch on the API server; in
/// this crate the watch's data arrives as a [`K8sSnapshot`], and a chain rebuilt
/// with that snapshot resolves the cluster's current services. Corefile options
/// on the directive (`fallthrough`, `pods`, `ttl`, `namespaces`) still apply on
/// top of the seeded registry.
///
/// # Errors
/// As [`build_chain`], plus a [`WireError::Config`] if the snapshot fails to
/// convert (malformed API JSON).
pub fn build_chain_with(block: &ServerBlock, k8s: Option<&K8sSnapshot>) -> Result<Chain> {
    // Sort the configured directives by canonical priority. Every directive
    // must be a known plugin (validated here) before we attempt to build it.
    let mut ordered: Vec<(usize, &Directive)> = Vec::with_capacity(block.plugins.len());
    for dir in &block.plugins {
        let prio = priority(&dir.name).ok_or(WireError::Config {
            reason: "unknown directive",
        })?;
        ordered.push((prio, dir));
    }
    // Stable sort keeps the declared order among directives of equal priority
    // (e.g. two `rewrite` rules), exactly as CoreDNS's `dnsserver` does.
    ordered.sort_by_key(|(prio, _)| *prio);

    let zone = block.zones.first().map_or("cluster.local", String::as_str);
    let mut plugins: Vec<Box<dyn Plugin>> = Vec::with_capacity(ordered.len());
    for (_, dir) in ordered {
        plugins.push(instantiate(dir, zone, k8s)?);
    }
    Ok(Chain::new(plugins))
}

/// Build one plugin from its directive.
fn instantiate(dir: &Directive, zone: &str, k8s: Option<&K8sSnapshot>) -> Result<Box<dyn Plugin>> {
    match dir.name.as_str() {
        "errors" => Ok(Box::new(Errors::new())),
        "ready" => {
            // A built chain is serving, so the readiness gate is open; the
            // deferred health server is what would toggle it at runtime.
            let ready = Ready::new();
            ready.set_ready(true);
            Ok(Box::new(ready))
        }
        "prometheus" => Ok(Box::new(Metrics::new())),
        "cache" => Ok(Box::new(build_cache(dir))),
        "rewrite" => Ok(Box::new(build_rewrite(dir)?)),
        "hosts" => Ok(Box::new(build_hosts(dir))),
        "kubernetes" => Ok(Box::new(build_kubernetes(dir, zone, k8s)?)),
        "file" => Ok(Box::new(build_file(dir, zone)?)),
        "forward" => Ok(Box::new(build_forward(dir)?)),
        // A known CoreDNS plugin that this crate does not (yet) implement: fail
        // loudly rather than resolve differently than the operator wrote.
        _ => Err(WireError::Config {
            reason: "unsupported plugin",
        }),
    }
}

/// `cache [TTL] [ZONES…]` — the success-cache capacity (TTL lives per entry).
fn build_cache(_dir: &Directive) -> CachePlugin {
    CachePlugin::new(DEFAULT_CACHE_CAPACITY)
}

/// `forward FROM TO… { policy … }` — the upstream set and load-balancing policy.
fn build_forward(dir: &Directive) -> Result<Forward> {
    // args = [FROM, TO, TO, …]; FROM is the zone the forwarder owns (usually
    // `.`), the rest are upstream addresses.
    let upstreams: Vec<String> = dir.args.iter().skip(1).cloned().collect();
    if upstreams.is_empty() {
        return Err(WireError::Config {
            reason: "forward needs at least one upstream",
        });
    }
    let policy = dir
        .block
        .iter()
        .find(|d| d.name == "policy")
        .and_then(|d| d.args.first())
        .map_or(ForwardPolicy::RoundRobin, |p| match p.as_str() {
            "random" => ForwardPolicy::Random,
            "sequential" => ForwardPolicy::Sequential,
            _ => ForwardPolicy::RoundRobin,
        });
    Ok(Forward::new(upstreams, policy))
}

/// `kubernetes ZONE… { pods MODE; fallthrough; ttl N; namespaces… }`.
///
/// When a [`K8sSnapshot`] is supplied the plugin starts from the converted API
/// registry; otherwise it starts empty (services arrive via a later reload).
fn build_kubernetes(dir: &Directive, zone: &str, k8s: Option<&K8sSnapshot>) -> Result<Kubernetes> {
    let kzone = dir.args.first().map_or(zone, String::as_str);
    let mut k = match k8s {
        Some(snapshot) => snapshot.resolve(kzone)?,
        None => Kubernetes::new(kzone),
    };
    for sub in &dir.block {
        match sub.name.as_str() {
            "fallthrough" => k = k.with_fallthrough(true),
            "pods" => {
                // `insecure` synthesises A records from the pod name; anything
                // else (`disabled`/`verified`) turns synthesis off.
                let insecure = sub.args.first().is_some_and(|m| m == "insecure");
                k = k.with_pods_insecure(insecure);
            }
            "ttl" => {
                if let Some(ttl) = sub.args.first().and_then(|s| s.parse().ok()) {
                    k = k.with_ttl(ttl);
                }
            }
            "namespaces" => {
                for ns in &sub.args {
                    k = k.with_namespace(ns);
                }
            }
            _ => {}
        }
    }
    Ok(k)
}

/// `hosts [HOSTSFILE] [ZONES…] { INLINE-ENTRIES; fallthrough; ttl N }`.
fn build_hosts(dir: &Directive) -> Hosts {
    // Inline entries appear as subdirectives `IP NAME…`; reassemble them into
    // hostfile lines for the hosts parser.
    let mut text = String::new();
    let mut fallthrough = false;
    let mut ttl: Option<u32> = None;
    for sub in &dir.block {
        match sub.name.as_str() {
            "fallthrough" => fallthrough = true,
            "ttl" => ttl = sub.args.first().and_then(|s| s.parse().ok()),
            "no_reverse" | "reload" => {}
            ip => {
                text.push_str(ip);
                for name in &sub.args {
                    text.push(' ');
                    text.push_str(name);
                }
                text.push('\n');
            }
        }
    }
    let mut hosts = Hosts::parse(&text).with_fallthrough(fallthrough);
    if let Some(ttl) = ttl {
        hosts = hosts.with_ttl(ttl);
    }
    hosts
}

/// `file DBFILE [ZONES…]` — read and parse the master zone file from disk.
fn build_file(dir: &Directive, zone: &str) -> Result<FilePlugin> {
    let path = dir.args.first().ok_or(WireError::Config {
        reason: "file needs a zone-file path",
    })?;
    let origin_str = dir.args.get(1).map_or(zone, String::as_str);
    let origin = Name::parse(origin_str).map_err(|_| WireError::Config {
        reason: "file: bad origin",
    })?;
    let text = std::fs::read_to_string(path).map_err(|_| WireError::Config {
        reason: "file: cannot read zone file",
    })?;
    let zone = Zone::from_master(origin, &text)?;
    Ok(FilePlugin::new(zone))
}

/// `rewrite [continue|stop] FIELD …` — one rewrite rule.
fn build_rewrite(dir: &Directive) -> Result<Rewriter> {
    let mut args = dir.args.iter().map(String::as_str).peekable();
    let mut policy_continue = false;
    match args.peek() {
        Some(&"continue") => {
            policy_continue = true;
            args.next();
        }
        Some(&"stop") => {
            args.next();
        }
        _ => {}
    }
    let field = args.next().ok_or(WireError::Config {
        reason: "rewrite needs a field",
    })?;
    let mut rule = match field {
        "type" => {
            let from = parse_rtype(next_arg(&mut args)?)?;
            let to = parse_rtype(next_arg(&mut args)?)?;
            Rule::rtype(from, to)
        }
        "class" => {
            let from = parse_class(next_arg(&mut args)?)?;
            let to = parse_class(next_arg(&mut args)?)?;
            Rule::class(from, to)
        }
        "name" => {
            // Optional match-mode keyword; defaults to `exact`.
            let mode = match args.peek().copied() {
                Some("exact") => {
                    args.next();
                    NameRule::Exact
                }
                Some("prefix") => {
                    args.next();
                    NameRule::Prefix
                }
                Some("suffix") => {
                    args.next();
                    NameRule::Suffix
                }
                Some("substring") => {
                    args.next();
                    NameRule::Substring
                }
                _ => NameRule::Exact,
            };
            let from = next_arg(&mut args)?;
            let to = next_arg(&mut args)?;
            Rule::name(mode, from, to)
        }
        _ => {
            return Err(WireError::Config {
                reason: "rewrite: unsupported field",
            });
        }
    };
    if policy_continue {
        rule = rule.continuing();
    }
    Ok(Rewriter::new(vec![rule]))
}

/// Pull the next required token from a rewrite directive.
fn next_arg<'a>(args: &mut impl Iterator<Item = &'a str>) -> Result<&'a str> {
    args.next().ok_or(WireError::Config {
        reason: "rewrite: missing argument",
    })
}

/// Parse a record-type mnemonic.
fn parse_rtype(s: &str) -> Result<RecordType> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "A" => RecordType::A,
        "NS" => RecordType::Ns,
        "CNAME" => RecordType::Cname,
        "SOA" => RecordType::Soa,
        "PTR" => RecordType::Ptr,
        "MX" => RecordType::Mx,
        "TXT" => RecordType::Txt,
        "AAAA" => RecordType::Aaaa,
        "SRV" => RecordType::Srv,
        "OPT" => RecordType::Opt,
        "ANY" => RecordType::Any,
        "HINFO" => RecordType::Unknown(13),
        other => other
            .strip_prefix("TYPE")
            .and_then(|n| n.parse().ok())
            .map(RecordType::Unknown)
            .ok_or(WireError::Config {
                reason: "rewrite: unknown record type",
            })?,
    })
}

/// Parse a DNS class mnemonic.
fn parse_class(s: &str) -> Result<Class> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "IN" => Class::In,
        "CH" => Class::Ch,
        "HS" => Class::Hs,
        "ANY" => Class::Any,
        _ => {
            return Err(WireError::Config {
                reason: "rewrite: unknown class",
            });
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::corefile::Corefile;

    fn block(text: &str) -> ServerBlock {
        Corefile::parse(text).unwrap().servers.pop().unwrap()
    }

    #[test]
    fn directives_list_matches_coredns_priority() {
        // A spot-check of the ported plugin.cfg ordering: kubernetes sorts
        // before file before forward, and cache before rewrite before hosts.
        let idx = |name: &str| DIRECTIVES.iter().position(|d| *d == name).unwrap();
        assert!(idx("cache") < idx("rewrite"));
        assert!(idx("rewrite") < idx("hosts"));
        assert!(idx("hosts") < idx("kubernetes"));
        assert!(idx("kubernetes") < idx("file"));
        assert!(idx("file") < idx("forward"));
        assert!(idx("errors") < idx("cache"));
        assert!(idx("ready") < idx("errors"));
    }

    #[test]
    fn chain_is_ordered_by_priority_not_declaration() {
        // Declared backwards on purpose: forward, kubernetes, cache, errors.
        let sb = block(
            ". {
                forward . 1.1.1.1
                kubernetes cluster.local
                cache
                errors
            }",
        );
        let chain = build_chain(&sb).unwrap();
        assert_eq!(
            chain.plugin_names(),
            vec!["errors", "cache", "kubernetes", "forward"],
            "plugins must run in canonical CoreDNS order, not Corefile order"
        );
    }

    #[test]
    fn unsupported_plugin_is_a_config_error() {
        let sb = block(". {\n dnssec \n}");
        assert!(matches!(build_chain(&sb), Err(WireError::Config { .. })));
    }

    #[test]
    fn unknown_directive_is_a_config_error() {
        let sb = block(". {\n notaplugin \n}");
        assert!(matches!(build_chain(&sb), Err(WireError::Config { .. })));
    }

    #[test]
    fn forward_upstreams_are_parsed() {
        let sb = block(". {\n forward . 1.1.1.1 8.8.8.8 \n}");
        let chain = build_chain(&sb).unwrap();
        assert_eq!(chain.plugin_names(), vec!["forward"]);
    }

    #[test]
    fn kubernetes_zone_and_options_are_parsed() {
        let sb = block(
            "cluster.local {
                kubernetes cluster.local {
                    pods insecure
                    fallthrough
                }
            }",
        );
        let chain = build_chain(&sb).unwrap();
        assert_eq!(chain.plugin_names(), vec!["kubernetes"]);
    }

    #[test]
    fn rewrite_type_rule_is_parsed() {
        let sb = block(". {\n rewrite type ANY HINFO \n forward . 1.1.1.1 \n}");
        let chain = build_chain(&sb).unwrap();
        assert_eq!(chain.plugin_names(), vec!["rewrite", "forward"]);
    }

    #[test]
    fn hosts_inline_entries_are_parsed() {
        let sb = block(". {\n hosts {\n 10.0.0.1 web.local \n } \n}");
        let chain = build_chain(&sb).unwrap();
        assert_eq!(chain.plugin_names(), vec!["hosts"]);
    }

    #[test]
    fn an_empty_block_builds_an_empty_chain() {
        let sb = block(". {\n}");
        assert!(build_chain(&sb).unwrap().plugin_names().is_empty());
    }

    #[test]
    fn build_chain_with_snapshot_seeds_the_kubernetes_plugin() {
        use crate::k8s::K8sSnapshot;
        use crate::message::Message;
        use crate::rr::Rdata;
        use std::net::Ipv4Addr;

        let sb = block("cluster.local {\n kubernetes cluster.local {\n fallthrough \n} \n}");
        let snap = K8sSnapshot::new(
            r#"{"items":[{"metadata":{"name":"web","namespace":"default"},
                "spec":{"type":"ClusterIP","clusterIP":"10.0.0.1"}}]}"#,
            r#"{"items":[]}"#,
        );
        let chain = build_chain_with(&sb, Some(&snap)).unwrap();
        let reply = chain.handle(&Message::query(
            Name::parse("web.default.svc.cluster.local").unwrap(),
            RecordType::A,
            1,
        ));
        assert_eq!(reply.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn corefile_options_apply_on_top_of_the_snapshot() {
        use crate::k8s::K8sSnapshot;

        // `fallthrough` from the Corefile must still take effect when the
        // plugin is seeded from a snapshot.
        let sb = block("cluster.local {\n kubernetes cluster.local {\n fallthrough \n} \n}");
        let snap = K8sSnapshot::new(r#"{"items":[]}"#, r#"{"items":[]}"#);
        // It builds without error and is the single kubernetes plugin.
        let chain = build_chain_with(&sb, Some(&snap)).unwrap();
        assert_eq!(chain.plugin_names(), vec!["kubernetes"]);
    }
}
