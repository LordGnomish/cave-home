// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Caddy-style `Corefile` parser.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_server_block() {
        let cfg = Corefile::parse(
            ". {
                forward . 1.1.1.1 8.8.8.8
                cache 30
                log
            }",
        )
        .unwrap();
        assert_eq!(cfg.servers.len(), 1);
        let s = &cfg.servers[0];
        assert_eq!(s.zones, vec!["."]);
        assert_eq!(s.port, 53);
        assert_eq!(s.plugins.len(), 3);
        assert_eq!(s.plugins[0].name, "forward");
        assert_eq!(s.plugins[0].args, vec![".", "1.1.1.1", "8.8.8.8"]);
        assert_eq!(s.plugins[1].name, "cache");
        assert_eq!(s.plugins[1].args, vec!["30"]);
        assert_eq!(s.plugins[2].name, "log");
        assert!(s.plugins[2].args.is_empty());
    }

    #[test]
    fn parses_zone_and_explicit_port() {
        let cfg = Corefile::parse("example.com:1053 {\n whoami \n}").unwrap();
        assert_eq!(cfg.servers[0].zones, vec!["example.com"]);
        assert_eq!(cfg.servers[0].port, 1053);
    }

    #[test]
    fn parses_multiple_zones_sharing_a_header() {
        let cfg = Corefile::parse("example.com example.org:5353 {\n whoami \n}").unwrap();
        assert_eq!(cfg.servers[0].zones, vec!["example.com", "example.org"]);
        assert_eq!(cfg.servers[0].port, 5353);
    }

    #[test]
    fn parses_nested_subdirectives() {
        let cfg = Corefile::parse(
            ". {
                kubernetes cluster.local {
                    pods insecure
                    fallthrough
                }
            }",
        )
        .unwrap();
        let k = &cfg.servers[0].plugins[0];
        assert_eq!(k.name, "kubernetes");
        assert_eq!(k.args, vec!["cluster.local"]);
        assert_eq!(k.block.len(), 2);
        assert_eq!(k.block[0].name, "pods");
        assert_eq!(k.block[0].args, vec!["insecure"]);
        assert_eq!(k.block[1].name, "fallthrough");
        assert!(k.block[1].args.is_empty());
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let cfg = Corefile::parse(
            "# top comment
            . {
                # inner comment
                whoami   # trailing comment

                errors
            }",
        )
        .unwrap();
        assert_eq!(cfg.servers[0].plugins.len(), 2);
        assert_eq!(cfg.servers[0].plugins[0].name, "whoami");
        assert_eq!(cfg.servers[0].plugins[1].name, "errors");
    }

    #[test]
    fn parses_multiple_server_blocks() {
        let cfg = Corefile::parse(
            ".:53 {
                forward . 1.1.1.1
            }
            cluster.local:5353 {
                kubernetes cluster.local
            }",
        )
        .unwrap();
        assert_eq!(cfg.servers.len(), 2);
        assert_eq!(cfg.servers[0].port, 53);
        assert_eq!(cfg.servers[1].zones, vec!["cluster.local"]);
        assert_eq!(cfg.servers[1].port, 5353);
    }

    #[test]
    fn quoted_arguments_keep_spaces() {
        let cfg = Corefile::parse(". {\n log \"a b c\" \n}").unwrap();
        assert_eq!(cfg.servers[0].plugins[0].args, vec!["a b c"]);
    }

    #[test]
    fn unbalanced_braces_are_an_error() {
        assert!(Corefile::parse(". {\n forward . 1.1.1.1 \n").is_err());
        assert!(Corefile::parse(". }").is_err());
    }

    #[test]
    fn finds_a_plugin_directive_by_name() {
        let cfg = Corefile::parse(". {\n cache 30 \n forward . 1.1.1.1 \n}").unwrap();
        assert_eq!(cfg.servers[0].plugin("cache").unwrap().args, vec!["30"]);
        assert!(cfg.servers[0].plugin("rewrite").is_none());
    }
}
