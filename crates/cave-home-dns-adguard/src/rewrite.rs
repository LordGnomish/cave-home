//! DNS rewrites: map a domain to a fixed local answer.
//!
//! Clean-room (Charter §6.1 / ADR-022): the rewrite *model* mirrors the publicly
//! documented "DNS rewrite" feature behaviour (point a name at a fixed address,
//! with the same domain-and-subdomain matching as filter rules) — not the
//! upstream source. The actual answer synthesis on the wire is Phase 1b; this is the
//! decision data the resolver will consume.
//!
//! A common household use is pinning a name to a LAN device, e.g.
//! `printer.home → 192.168.1.50`, or sink-holing a name to a local block page.

use crate::rule::{DomainPattern, normalize_domain};

/// The fixed answer a rewrite resolves a domain to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A literal IPv4 address, e.g. `192.168.1.50`.
    Ipv4([u8; 4]),
    /// A literal IPv6 address (kept as the eight 16-bit groups).
    Ipv6([u16; 8]),
    /// Resolve to another name instead (a `CNAME`-like alias).
    Alias(String),
}

/// One rewrite: a matching pattern plus the answer to return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pattern: DomainPattern,
    answer: Answer,
}

impl Rewrite {
    /// Point a domain (and its subdomains) at a fixed answer.
    #[must_use]
    pub fn new(domain: impl AsRef<str>, answer: Answer) -> Self {
        Self {
            pattern: DomainPattern::Subdomain(normalize_domain(domain.as_ref())),
            answer,
        }
    }

    /// Point only the exact domain (no subdomains) at a fixed answer.
    #[must_use]
    pub fn exact(domain: impl AsRef<str>, answer: Answer) -> Self {
        Self {
            pattern: DomainPattern::Exact(normalize_domain(domain.as_ref())),
            answer,
        }
    }

    #[must_use]
    pub const fn answer(&self) -> &Answer {
        &self.answer
    }

    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        self.pattern.matches(query)
    }
}

/// An ordered table of rewrites. First match (by insertion order) wins, so a
/// more-specific exact rewrite can be listed ahead of a broad subdomain one.
#[derive(Debug, Clone, Default)]
pub struct RewriteTable {
    rewrites: Vec<Rewrite>,
}

impl RewriteTable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rewrites: Vec::new(),
        }
    }

    /// Add a rewrite to the end of the table.
    pub fn add(&mut self, rewrite: Rewrite) {
        self.rewrites.push(rewrite);
    }

    /// Resolve a query to its rewritten answer, if any rewrite matches.
    ///
    /// The query is normalised here so callers can pass raw lookups.
    #[must_use]
    pub fn resolve(&self, query: &str) -> Option<&Answer> {
        let q = normalize_domain(query);
        self.rewrites
            .iter()
            .find(|rw| rw.matches(&q))
            .map(Rewrite::answer)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rewrites.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rewrites.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomain_rewrite_covers_children() {
        let rw = Rewrite::new("home", Answer::Ipv4([192, 168, 1, 1]));
        assert!(rw.matches("home"));
        assert!(rw.matches("printer.home"));
    }

    #[test]
    fn exact_rewrite_does_not_cover_children() {
        let rw = Rewrite::exact("printer.home", Answer::Ipv4([192, 168, 1, 50]));
        assert!(rw.matches("printer.home"));
        assert!(!rw.matches("scan.printer.home"));
    }

    #[test]
    fn table_resolves_and_normalises_query() {
        let mut t = RewriteTable::new();
        t.add(Rewrite::new(
            "Printer.Home",
            Answer::Ipv4([192, 168, 1, 50]),
        ));
        assert_eq!(
            t.resolve("PRINTER.HOME."),
            Some(&Answer::Ipv4([192, 168, 1, 50]))
        );
        assert_eq!(t.resolve("other.lan"), None);
    }

    #[test]
    fn first_match_wins_for_specificity() {
        let mut t = RewriteTable::new();
        t.add(Rewrite::exact(
            "api.example.com",
            Answer::Ipv4([10, 0, 0, 1]),
        ));
        t.add(Rewrite::new("example.com", Answer::Ipv4([10, 0, 0, 2])));
        // Exact listed first wins for the exact name…
        assert_eq!(
            t.resolve("api.example.com"),
            Some(&Answer::Ipv4([10, 0, 0, 1]))
        );
        // …while other subdomains fall through to the broad rewrite.
        assert_eq!(
            t.resolve("www.example.com"),
            Some(&Answer::Ipv4([10, 0, 0, 2]))
        );
    }

    #[test]
    fn alias_and_ipv6_answers_round_trip() {
        let mut t = RewriteTable::new();
        t.add(Rewrite::new(
            "old.example.com",
            Answer::Alias("new.example.com".into()),
        ));
        t.add(Rewrite::new(
            "v6.example.com",
            Answer::Ipv6([0xfd00, 0, 0, 0, 0, 0, 0, 1]),
        ));
        assert_eq!(
            t.resolve("old.example.com"),
            Some(&Answer::Alias("new.example.com".into()))
        );
        assert_eq!(
            t.resolve("v6.example.com"),
            Some(&Answer::Ipv6([0xfd00, 0, 0, 0, 0, 0, 0, 1]))
        );
    }

    #[test]
    fn empty_table_resolves_nothing() {
        let t = RewriteTable::new();
        assert!(t.is_empty());
        assert_eq!(t.resolve("anything.example.com"), None);
    }
}
