//! Blocklist parsing for the common *public* formats.
//!
//! Clean-room (Charter §6.1 / ADR-022): implemented from the publicly documented
//! syntaxes only — never from the upstream ad-blocker's GPL source.
//!
//! Supported line shapes:
//!
//! - **`/etc/hosts` style**: `0.0.0.0 ads.example.com` (or `127.0.0.1 …`). The
//!   IP is the sink address; only the domain matters to the filter. Treated as
//!   a domain-and-subdomains block.
//! - **Adblock-style domain rules**:
//!   - `||ads.example.com^` — block the domain and its subdomains.
//!   - `@@||good.example.com^` — allow (exception), beats any block rule.
//!   - `|example.com|` — exact-only anchor (no subdomain match).
//!   - A trailing `^` separator is accepted and stripped.
//! - **Plain domain lists**: a bare `ads.example.com` per line, blocked with
//!   subdomain semantics (the dominant blocklist convention).
//! - **Comments** (`#` or `!`) and blank lines are ignored.
//!
//! Modifiers (the `$...` Adblock options such as `$third-party`, `$important`)
//! are **not** evaluated in Phase 1 — a rule carrying any modifier is skipped
//! rather than mis-applied, and this is recorded in the parity manifest. The
//! parser never panics on malformed input: an unparseable line is skipped.

use crate::rule::{Action, DomainPattern, Rule, is_plausible_domain, normalize_domain};

/// Parse a whole blocklist (newline-separated) into rules, skipping comments,
/// blanks and anything malformed.
#[must_use]
pub fn parse_blocklist(text: &str) -> Vec<Rule> {
    text.lines().filter_map(parse_line).collect()
}

/// Parse a single blocklist line into a [`Rule`], or `None` if the line is a
/// comment, blank, or not something we can safely compile.
#[must_use]
pub fn parse_line(line: &str) -> Option<Rule> {
    let trimmed = line.trim();
    if trimmed.is_empty() || is_comment(trimmed) {
        return None;
    }

    // Adblock `$modifier` options are not evaluated in Phase 1: skip rather
    // than silently mis-apply (e.g. dropping a `$third-party`-only rule's scope).
    if trimmed.contains('$') {
        return None;
    }

    if let Some(rule) = parse_hosts_line(trimmed) {
        return Some(rule);
    }
    if let Some(rule) = parse_adblock_line(trimmed) {
        return Some(rule);
    }
    parse_plain_domain(trimmed)
}

/// `#` (hosts/dnsmasq) and `!` (Adblock) start a comment line.
fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with('#') || trimmed.starts_with('!')
}

/// `/etc/hosts` style: `<ip> <domain>` (optionally more whitespace / trailing
/// comment). Only the sink IPs commonly used for blackholing are accepted, so
/// a genuine host mapping like `192.168.1.10 nas` is not mistaken for a block.
fn parse_hosts_line(trimmed: &str) -> Option<Rule> {
    let mut parts = trimmed.split_whitespace();
    let ip = parts.next()?;
    if !is_sink_ip(ip) {
        return None;
    }
    let domain_raw = parts.next()?;
    // Any further token must be the start of a trailing comment; a bare extra
    // token (e.g. `0.0.0.0 exa mple`) means the line is malformed — skip it.
    if let Some(next) = parts.next() {
        if !next.starts_with('#') {
            return None;
        }
    }
    let domain = normalize_domain(domain_raw);
    if !is_plausible_domain(&domain) {
        return None;
    }
    Some(Rule::new(
        Action::Block,
        DomainPattern::Subdomain(domain),
        trimmed,
    ))
}

/// The conventional blackhole sink addresses used in hosts-style blocklists.
fn is_sink_ip(ip: &str) -> bool {
    matches!(ip, "0.0.0.0" | "127.0.0.1" | "::" | "::1")
}

/// Adblock-style domain rule. Returns `None` if the line is not in this family
/// or carries a path/regex we do not model in Phase 1.
fn parse_adblock_line(trimmed: &str) -> Option<Rule> {
    let (action, rest) = trimmed
        .strip_prefix("@@")
        .map_or((Action::Block, trimmed), |r| (Action::Allow, r));

    // The domain-anchor family always starts with `||` or a single `|`.
    if !rest.starts_with('|') {
        return None;
    }

    // `||domain^` — host-and-subdomains.
    if let Some(after) = rest.strip_prefix("||") {
        let body = after.trim_end_matches('^');
        let domain = normalize_domain(body);
        if !is_plausible_domain(&domain) {
            return None;
        }
        return Some(Rule::new(action, DomainPattern::Subdomain(domain), trimmed));
    }

    // `|domain|` — fully anchored exact match (no subdomains).
    if let Some(after) = rest.strip_prefix('|') {
        let body = after.trim_end_matches('^');
        let body = body.strip_suffix('|').unwrap_or(body);
        let domain = normalize_domain(body);
        if !is_plausible_domain(&domain) {
            return None;
        }
        return Some(Rule::new(action, DomainPattern::Exact(domain), trimmed));
    }

    None
}

/// A bare domain on its own line → block with subdomain semantics.
fn parse_plain_domain(trimmed: &str) -> Option<Rule> {
    // Reject anything that smells like a URL / path / wildcard we do not model.
    if trimmed.contains('/') || trimmed.contains('*') || trimmed.contains(' ') {
        return None;
    }
    let domain = normalize_domain(trimmed);
    if !is_plausible_domain(&domain) {
        return None;
    }
    // A bare IP literal (e.g. a lone `0.0.0.0`) is not a domain to block; a real
    // domain has at least one non-numeric label.
    if domain
        .split('.')
        .all(|l| l.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(Rule::new(
        Action::Block,
        DomainPattern::Subdomain(domain),
        trimmed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_style_with_zero_sink() {
        let r = parse_line("0.0.0.0 ads.example.com").expect("hosts line");
        assert_eq!(r.action(), Action::Block);
        assert_eq!(
            r.pattern(),
            &DomainPattern::Subdomain("ads.example.com".into())
        );
    }

    #[test]
    fn parses_hosts_style_with_loopback_sink() {
        let r = parse_line("127.0.0.1\ttracker.example.net").expect("hosts line");
        assert_eq!(r.pattern().domain(), "tracker.example.net");
    }

    #[test]
    fn ignores_real_host_mapping_not_a_sink() {
        // A genuine LAN host mapping is not a blocklist entry.
        assert!(parse_line("192.168.1.10 nas.lan").is_none());
    }

    #[test]
    fn parses_adblock_subdomain_block() {
        let r = parse_line("||ads.example.com^").expect("adblock line");
        assert_eq!(r.action(), Action::Block);
        assert_eq!(
            r.pattern(),
            &DomainPattern::Subdomain("ads.example.com".into())
        );
    }

    #[test]
    fn parses_adblock_allow_exception() {
        let r = parse_line("@@||good.example.com^").expect("allow line");
        assert_eq!(r.action(), Action::Allow);
        assert_eq!(
            r.pattern(),
            &DomainPattern::Subdomain("good.example.com".into())
        );
    }

    #[test]
    fn parses_adblock_exact_anchor() {
        let r = parse_line("|example.com|").expect("exact line");
        assert_eq!(r.pattern(), &DomainPattern::Exact("example.com".into()));
        assert!(r.matches("example.com"));
        assert!(!r.matches("ads.example.com"));
    }

    #[test]
    fn parses_plain_domain_as_subdomain_block() {
        let r = parse_line("doubleclick.net").expect("plain domain");
        assert_eq!(r.action(), Action::Block);
        assert!(r.matches("ad.doubleclick.net"));
    }

    #[test]
    fn ignores_comments_and_blanks() {
        assert!(parse_line("# this is a hosts comment").is_none());
        assert!(parse_line("! this is an adblock comment").is_none());
        assert!(parse_line("").is_none());
        assert!(parse_line("    ").is_none());
    }

    #[test]
    fn skips_modifier_rules_in_phase_1() {
        // We do not model `$third-party` etc., so the rule is skipped, not
        // applied with the wrong scope.
        assert!(parse_line("||ads.example.com^$third-party").is_none());
        assert!(parse_line("||x.example.com^$important").is_none());
    }

    #[test]
    fn skips_malformed_without_panicking() {
        assert!(parse_line("0.0.0.0").is_none()); // no domain
        assert!(parse_line("||^").is_none()); // empty domain
        assert!(parse_line("http://example.com/path").is_none()); // url/path
        assert!(parse_line("*.example.com").is_none()); // wildcard
        assert!(parse_line("0.0.0.0 exa mple").is_none()); // implausible
    }

    #[test]
    fn parse_blocklist_handles_mixed_document() {
        let doc = "\
# Title comment
! adblock comment

0.0.0.0 ads.example.com
||tracker.example.net^
@@||cdn.example.org^
plaindomain.example
|exact.example|
||bad.example^$popup
http://nope.example/x
";
        let rules = parse_blocklist(doc);
        // 4 blocks (hosts, adblock subdomain, plain, exact) + 1 allow = 5.
        assert_eq!(rules.len(), 5);
        let allows = rules.iter().filter(|r| r.action() == Action::Allow).count();
        assert_eq!(allows, 1);
    }

    #[test]
    fn hosts_line_trailing_comment_ignored() {
        let r = parse_line("0.0.0.0 ads.example.com # inline note").expect("hosts");
        assert_eq!(r.pattern().domain(), "ads.example.com");
    }
}
