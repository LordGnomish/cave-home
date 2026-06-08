//! The filter-rule model.
//!
//! Clean-room (Charter §6.1 / ADR-022): the rule shapes here are implemented
//! from the *public* blocklist-syntax documentation — Adblock-style domain
//! rules, `/etc/hosts` lines, and plain domain lists. The upstream ad-blocker's
//! GPL source was not read.
//!
//! A [`Rule`] is the compiled, normalised form of one blocklist line: what it
//! matches (a [`DomainPattern`]) and what it decides ([`Action`]).

/// What a rule does when it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Drop the query — an ad/tracker domain.
    Block,
    /// Force-allow the query, overriding any block rule. The Adblock `@@`
    /// exception prefix and the household's own "always allow" list compile to
    /// this.
    Allow,
}

/// How a rule's domain matches an incoming query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainPattern {
    /// Matches the domain itself *and* any subdomain of it. This is the
    /// `||example.com^` Adblock semantics and how `/etc/hosts` / plain-list
    /// entries are treated by AdGuard-class blockers: a list entry for
    /// `ads.example.com` also covers `t.ads.example.com`.
    Subdomain(String),
    /// Matches only the exact domain, never a subdomain. This is the
    /// fully-anchored `|example.com|` Adblock form.
    Exact(String),
}

impl DomainPattern {
    /// Does this pattern match the given already-normalised query domain?
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        match self {
            Self::Exact(d) => query == d,
            Self::Subdomain(d) => {
                if query == d {
                    return true;
                }
                // `query` must end with `.d` for it to be a subdomain of `d`,
                // so that `notexample.com` does not match `example.com`.
                query.len() > d.len()
                    && query.ends_with(d.as_str())
                    && query.as_bytes()[query.len() - d.len() - 1] == b'.'
            }
        }
    }

    /// The bare domain string this pattern is built around.
    #[must_use]
    pub fn domain(&self) -> &str {
        match self {
            Self::Exact(d) | Self::Subdomain(d) => d,
        }
    }
}

/// A compiled filter rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    action: Action,
    pattern: DomainPattern,
    /// The original source line, kept verbatim so the UI / audit log can show
    /// "blocked by <this list line>" without re-serialising.
    source: String,
}

impl Rule {
    /// Build a rule from its parts. `source` is the verbatim originating line.
    #[must_use]
    pub fn new(action: Action, pattern: DomainPattern, source: impl Into<String>) -> Self {
        Self {
            action,
            pattern,
            source: source.into(),
        }
    }

    /// A blocking rule covering a domain and its subdomains.
    #[must_use]
    pub fn block_subdomain(domain: impl Into<String>) -> Self {
        let d = domain.into();
        Self::new(Action::Block, DomainPattern::Subdomain(d.clone()), d)
    }

    /// An allow (exception) rule covering a domain and its subdomains.
    #[must_use]
    pub fn allow_subdomain(domain: impl Into<String>) -> Self {
        let d = domain.into();
        Self::new(Action::Allow, DomainPattern::Subdomain(d.clone()), d)
    }

    #[must_use]
    pub const fn action(&self) -> Action {
        self.action
    }

    #[must_use]
    pub const fn pattern(&self) -> &DomainPattern {
        &self.pattern
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Does this rule apply to the (already-normalised) query domain?
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        self.pattern.matches(query)
    }
}

/// Normalise a domain for matching.
///
/// Trims surrounding whitespace, drops a single trailing dot (the DNS root
/// label), and lowercases ASCII.
///
/// IDNA/punycode is intentionally left as raw bytes — an already-encoded
/// `xn--...` label compares fine; Unicode normalisation is a Phase-1b concern
/// (see the parity manifest).
#[must_use]
pub fn normalize_domain(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('.');
    trimmed.to_ascii_lowercase()
}

/// Is this a syntactically plausible domain label sequence?
///
/// Used by the parser to skip malformed entries instead of compiling garbage
/// rules. Conservative on purpose: must be non-empty, contain only DNS-legal
/// characters, and have no empty labels.
#[must_use]
pub fn is_plausible_domain(d: &str) -> bool {
    if d.is_empty() || d.len() > 253 {
        return false;
    }
    let mut labels = 0usize;
    for label in d.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        labels += 1;
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
    }
    labels >= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomain_pattern_matches_self_and_children() {
        let p = DomainPattern::Subdomain("example.com".into());
        assert!(p.matches("example.com"));
        assert!(p.matches("ads.example.com"));
        assert!(p.matches("a.b.example.com"));
    }

    #[test]
    fn subdomain_pattern_does_not_match_suffix_lookalikes() {
        let p = DomainPattern::Subdomain("example.com".into());
        assert!(!p.matches("notexample.com"));
        assert!(!p.matches("badexample.com"));
        assert!(!p.matches("example.com.evil.net"));
    }

    #[test]
    fn exact_pattern_matches_only_itself() {
        let p = DomainPattern::Exact("example.com".into());
        assert!(p.matches("example.com"));
        assert!(!p.matches("ads.example.com"));
        assert!(!p.matches("notexample.com"));
    }

    #[test]
    fn normalize_lowercases_and_strips_trailing_dot() {
        assert_eq!(normalize_domain("Ads.Example.COM."), "ads.example.com");
        assert_eq!(normalize_domain("  EXAMPLE.com  "), "example.com");
        assert_eq!(normalize_domain("example.com"), "example.com");
    }

    #[test]
    fn plausible_domain_accepts_real_domains() {
        assert!(is_plausible_domain("example.com"));
        assert!(is_plausible_domain("ads.tracker.co.uk"));
        assert!(is_plausible_domain("a-b.example.com"));
        assert!(is_plausible_domain("xn--mnchen-3ya.de"));
    }

    #[test]
    fn plausible_domain_rejects_garbage() {
        assert!(!is_plausible_domain(""));
        assert!(!is_plausible_domain(".example.com"));
        assert!(!is_plausible_domain("example..com"));
        assert!(!is_plausible_domain("exa mple.com"));
        assert!(!is_plausible_domain("-bad.example.com"));
        assert!(!is_plausible_domain("bad-.example.com"));
    }

    #[test]
    fn rule_constructors_keep_source_and_action() {
        let r = Rule::block_subdomain("ads.example.com");
        assert_eq!(r.action(), Action::Block);
        assert_eq!(r.pattern().domain(), "ads.example.com");
        assert_eq!(r.source(), "ads.example.com");
        assert!(r.matches("x.ads.example.com"));
    }
}
