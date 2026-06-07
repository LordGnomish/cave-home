// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Caddy-style `Corefile` parser.
//!
//! A `Corefile` is a list of *server blocks*: a header of `zone[:port]` tokens
//! followed by a `{ … }` body of plugin directives. A directive is a name, its
//! arguments, and an optional nested `{ … }` of subdirectives (e.g. the
//! `kubernetes` block). Tokens are whitespace-separated, `#` starts a comment
//! to end-of-line, and `"…"` groups an argument containing spaces — the same
//! grammar `CoreDNS`/Caddy use.
//!
//! This produces the config AST. Translating an AST into a live
//! [`crate::plugin::Chain`] (instantiating each plugin from its directive) is
//! the deferred wiring step (`parity.manifest.toml`).

use crate::error::{Result, WireError};

/// The default DNS port when a zone omits `:port`.
pub const DEFAULT_PORT: u16 = 53;

/// A parsed `Corefile`: an ordered list of server blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corefile {
    /// The server blocks in file order.
    pub servers: Vec<ServerBlock>,
}

/// One server block: the zones it serves, the listen port, and its plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerBlock {
    /// The zone names from the header.
    pub zones: Vec<String>,
    /// The listen port (default [`DEFAULT_PORT`]).
    pub port: u16,
    /// The plugin directives in declared order.
    pub plugins: Vec<Directive>,
}

impl ServerBlock {
    /// The first directive with the given name, if present.
    #[must_use]
    pub fn plugin(&self, name: &str) -> Option<&Directive> {
        self.plugins.iter().find(|d| d.name == name)
    }
}

/// A plugin directive: a name, its arguments, and any nested subdirectives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// The directive (plugin) name.
    pub name: String,
    /// The positional arguments.
    pub args: Vec<String>,
    /// Nested subdirectives from a `{ … }` block, if any.
    pub block: Vec<Self>,
}

/// A lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Open,
    Close,
    Newline,
}

impl Corefile {
    /// Parse `Corefile` text into the config AST.
    ///
    /// # Errors
    /// [`WireError::Corefile`] on a missing block, an unexpected token, or
    /// unbalanced braces.
    pub fn parse(text: &str) -> Result<Self> {
        let tokens = tokenize(text);
        let mut p = 0usize;
        let mut servers = Vec::new();

        loop {
            skip_newlines(&tokens, &mut p);
            if p >= tokens.len() {
                break;
            }
            // Server header: words up to the opening brace.
            let mut header = Vec::new();
            while let Some(tok) = tokens.get(p) {
                match tok {
                    Token::Word(w) => {
                        header.push(w.clone());
                        p += 1;
                    }
                    Token::Newline => p += 1,
                    Token::Open => break,
                    Token::Close => {
                        return Err(WireError::Corefile { reason: "unexpected '}'" });
                    }
                }
            }
            if header.is_empty() || tokens.get(p) != Some(&Token::Open) {
                return Err(WireError::Corefile { reason: "expected '{' after server header" });
            }
            p += 1; // consume Open
            let plugins = parse_block(&tokens, &mut p)?;
            servers.push(server_from_header(&header, plugins)?);
        }
        Ok(Self { servers })
    }
}

/// Parse a `{ … }` body (the opening brace already consumed).
fn parse_block(tokens: &[Token], p: &mut usize) -> Result<Vec<Directive>> {
    let mut dirs = Vec::new();
    loop {
        skip_newlines(tokens, p);
        match tokens.get(*p) {
            None => return Err(WireError::Corefile { reason: "unbalanced '{'" }),
            Some(Token::Close) => {
                *p += 1;
                return Ok(dirs);
            }
            Some(Token::Open) => return Err(WireError::Corefile { reason: "unexpected '{'" }),
            Some(Token::Word(name)) => {
                let name = name.clone();
                *p += 1;
                let (args, block) = parse_directive_tail(tokens, p)?;
                dirs.push(Directive { name, args, block });
            }
            Some(Token::Newline) => *p += 1,
        }
    }
}

/// Parse a directive's arguments and optional nested block.
fn parse_directive_tail(tokens: &[Token], p: &mut usize) -> Result<(Vec<String>, Vec<Directive>)> {
    let mut args = Vec::new();
    loop {
        match tokens.get(*p) {
            Some(Token::Word(w)) => {
                args.push(w.clone());
                *p += 1;
            }
            Some(Token::Open) => {
                *p += 1;
                let block = parse_block(tokens, p)?;
                return Ok((args, block));
            }
            Some(Token::Newline) => {
                *p += 1;
                return Ok((args, Vec::new()));
            }
            Some(Token::Close) | None => return Ok((args, Vec::new())),
        }
    }
}

/// Build a [`ServerBlock`] from its header words.
fn server_from_header(header: &[String], plugins: Vec<Directive>) -> Result<ServerBlock> {
    let mut zones = Vec::new();
    let mut port = None;
    for raw in header {
        // Strip an optional transport scheme (dns:// / tls:// / …).
        let addr = raw.split_once("://").map_or(raw.as_str(), |(_, rest)| rest);
        let (zone, addr_port) = addr.rsplit_once(':').map_or((addr, None), |(z, pstr)| (z, Some(pstr)));
        if let Some(pstr) = addr_port {
            let parsed = pstr.parse().map_err(|_| WireError::Corefile { reason: "bad port" })?;
            port = Some(parsed);
        }
        zones.push(zone.to_string());
    }
    Ok(ServerBlock { zones, port: port.unwrap_or(DEFAULT_PORT), plugins })
}

/// Advance `p` past any run of newline tokens.
fn skip_newlines(tokens: &[Token], p: &mut usize) {
    while tokens.get(*p) == Some(&Token::Newline) {
        *p += 1;
    }
}

/// Lex `Corefile` text into tokens, dropping comments.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\r' => {
                chars.next();
            }
            '\n' => {
                chars.next();
                tokens.push(Token::Newline);
            }
            '#' => {
                while let Some(&nc) = chars.peek() {
                    if nc == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '{' => {
                chars.next();
                tokens.push(Token::Open);
            }
            '}' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '"' {
                        break;
                    }
                    s.push(nc);
                }
                tokens.push(Token::Word(s));
            }
            _ => {
                let mut s = String::new();
                while let Some(&nc) = chars.peek() {
                    if matches!(nc, ' ' | '\t' | '\r' | '\n' | '{' | '}' | '#' | '"') {
                        break;
                    }
                    s.push(nc);
                    chars.next();
                }
                tokens.push(Token::Word(s));
            }
        }
    }
    tokens
}

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
