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
use crate::kubernetes::Kubernetes;
use crate::name::Name;
use crate::plugin::{Chain, Plugin};
use crate::rewrite::{NameRule, Rewriter, Rule};
use crate::rr::{Class, RecordType};

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
        assert!(matches!(
            build_chain(&sb),
            Err(WireError::Config { .. })
        ));
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
}
