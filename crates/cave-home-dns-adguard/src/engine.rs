//! The filter decision core: compile many rules into a [`RuleSet`] and decide
//! what happens to a query.
//!
//! Clean-room (Charter §6.1 / ADR-022): the decision *semantics* — allowlist
//! exceptions beat block rules, domain-and-subdomain matching, custom per-client
//! overrides — follow the publicly documented blocklist behaviour, not the
//! upstream GPL source.

use crate::parse::parse_blocklist;
use crate::rule::{Action, Rule, normalize_domain};

/// The verdict for one query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterDecision {
    /// The query is dropped; carries the rule that blocked it.
    Blocked(Rule),
    /// The query is force-allowed; carries the allow rule that let it through.
    Allowed(Rule),
    /// No rule touched the query — resolve it normally.
    NotFiltered,
}

impl FilterDecision {
    /// Convenience predicate for "should this query be dropped?".
    #[must_use]
    pub const fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked(_))
    }

    /// The rule behind this decision, if any.
    #[must_use]
    pub const fn rule(&self) -> Option<&Rule> {
        match self {
            Self::Blocked(r) | Self::Allowed(r) => Some(r),
            Self::NotFiltered => None,
        }
    }
}

/// A compiled set of filter rules plus the household's own client overrides.
///
/// Decision order (allowlist always wins):
/// 1. Client allow overrides (the household's "always allow this").
/// 2. Client block overrides (the household's "always block this").
/// 3. Blocklist allow exceptions (`@@` rules).
/// 4. Blocklist block rules.
/// 5. Otherwise [`FilterDecision::NotFiltered`].
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    /// Rules compiled from imported blocklists.
    list_rules: Vec<Rule>,
    /// The household's own per-domain overrides, checked before the lists.
    client_rules: Vec<Rule>,
}

impl RuleSet {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            list_rules: Vec::new(),
            client_rules: Vec::new(),
        }
    }

    /// Compile a blocklist document and add its rules.
    pub fn add_blocklist(&mut self, text: &str) {
        self.list_rules.extend(parse_blocklist(text));
    }

    /// Add already-built blocklist rules.
    pub fn add_rules(&mut self, rules: impl IntoIterator<Item = Rule>) {
        self.list_rules.extend(rules);
    }

    /// Add a household client override (an allow or block this household set by
    /// hand). These take precedence over the imported lists.
    pub fn add_client_rule(&mut self, rule: Rule) {
        self.client_rules.push(rule);
    }

    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.list_rules.len() + self.client_rules.len()
    }

    /// Decide what to do with a query domain.
    #[must_use]
    pub fn decide(&self, query: &str) -> FilterDecision {
        let q = normalize_domain(query);

        // 1 & 2 — household overrides first, allow before block.
        if let Some(r) = first_match(&self.client_rules, &q, Action::Allow) {
            return FilterDecision::Allowed(r.clone());
        }
        if let Some(r) = first_match(&self.client_rules, &q, Action::Block) {
            return FilterDecision::Blocked(r.clone());
        }

        // 3 — blocklist exceptions beat blocklist blocks.
        if let Some(r) = first_match(&self.list_rules, &q, Action::Allow) {
            return FilterDecision::Allowed(r.clone());
        }
        // 4 — blocklist blocks.
        if let Some(r) = first_match(&self.list_rules, &q, Action::Block) {
            return FilterDecision::Blocked(r.clone());
        }

        FilterDecision::NotFiltered
    }
}

/// Find the first rule in `rules` with the wanted action that matches `query`.
fn first_match<'a>(rules: &'a [Rule], query: &str, want: Action) -> Option<&'a Rule> {
    rules
        .iter()
        .find(|r| r.action() == want && r.matches(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruleset(doc: &str) -> RuleSet {
        let mut rs = RuleSet::new();
        rs.add_blocklist(doc);
        rs
    }

    #[test]
    fn blocks_listed_domain_and_subdomains() {
        let rs = ruleset("||ads.example.com^\n");
        assert!(rs.decide("ads.example.com").is_blocked());
        assert!(rs.decide("img.ads.example.com").is_blocked());
        assert_eq!(rs.decide("safe.example.com"), FilterDecision::NotFiltered);
    }

    #[test]
    fn exact_rule_does_not_block_subdomains() {
        let rs = ruleset("|tracker.example.com|\n");
        assert!(rs.decide("tracker.example.com").is_blocked());
        assert_eq!(
            rs.decide("sub.tracker.example.com"),
            FilterDecision::NotFiltered
        );
    }

    #[test]
    fn allowlist_exception_beats_block() {
        let rs = ruleset("||example.com^\n@@||good.example.com^\n");
        // The broad block covers example.com…
        assert!(rs.decide("ads.example.com").is_blocked());
        // …but the @@ exception wins for the allowed subdomain.
        let d = rs.decide("good.example.com");
        assert!(matches!(d, FilterDecision::Allowed(_)));
        assert!(rs.decide("api.good.example.com").rule().is_some());
    }

    #[test]
    fn client_allow_override_beats_blocklist_block() {
        let mut rs = ruleset("||example.com^\n");
        rs.add_client_rule(Rule::allow_subdomain("example.com"));
        assert!(matches!(
            rs.decide("ads.example.com"),
            FilterDecision::Allowed(_)
        ));
    }

    #[test]
    fn client_block_override_blocks_otherwise_clean_domain() {
        let mut rs = RuleSet::new();
        rs.add_client_rule(Rule::block_subdomain("timewaster.example"));
        assert!(rs.decide("www.timewaster.example").is_blocked());
        assert_eq!(rs.decide("other.example"), FilterDecision::NotFiltered);
    }

    #[test]
    fn client_allow_beats_client_block_for_same_domain() {
        let mut rs = RuleSet::new();
        rs.add_client_rule(Rule::block_subdomain("example.com"));
        rs.add_client_rule(Rule::allow_subdomain("example.com"));
        // Allow is evaluated before block within client overrides.
        assert!(matches!(
            rs.decide("example.com"),
            FilterDecision::Allowed(_)
        ));
    }

    #[test]
    fn decision_carries_the_originating_rule() {
        let rs = ruleset("0.0.0.0 ads.example.com\n");
        match rs.decide("ads.example.com") {
            FilterDecision::Blocked(r) => assert_eq!(r.source(), "0.0.0.0 ads.example.com"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn unrelated_domain_is_not_filtered() {
        let rs = ruleset("||ads.example.com^\n");
        assert_eq!(rs.decide("wikipedia.org"), FilterDecision::NotFiltered);
    }

    #[test]
    fn decide_normalises_query_case_and_trailing_dot() {
        let rs = ruleset("||ads.example.com^\n");
        assert!(rs.decide("ADS.Example.COM.").is_blocked());
    }

    #[test]
    fn rule_count_tracks_both_sources() {
        let mut rs = ruleset("||a.example.com^\n||b.example.com^\n");
        rs.add_client_rule(Rule::allow_subdomain("c.example.com"));
        assert_eq!(rs.rule_count(), 3);
    }
}
