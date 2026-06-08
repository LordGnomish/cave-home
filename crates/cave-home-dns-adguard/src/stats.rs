//! Query statistics: pure aggregation over a passed-in query log.
//!
//! Clean-room (Charter §6.1 / ADR-022): the headline counters (total queries,
//! blocked queries, top blocked domains) mirror the publicly documented
//! dashboard figures, computed here over an in-memory log. No live query
//! capture — that interception loop is Phase 1b (see the parity manifest).

use crate::engine::{FilterDecision, RuleSet};
use std::collections::HashMap;

/// One observed lookup: the domain that was queried and what the engine decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryEvent {
    pub domain: String,
    pub blocked: bool,
}

impl QueryEvent {
    #[must_use]
    pub fn new(domain: impl Into<String>, blocked: bool) -> Self {
        Self {
            domain: domain.into(),
            blocked,
        }
    }
}

/// Aggregated counters over a query log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stats {
    /// Total queries seen.
    pub total: u64,
    /// How many of those were blocked.
    pub blocked: u64,
    /// Per-domain query counts.
    pub per_domain: HashMap<String, u64>,
    /// Per-domain blocked counts.
    pub blocked_per_domain: HashMap<String, u64>,
}

impl Stats {
    /// Aggregate a log of already-decided events.
    #[must_use]
    pub fn from_events(events: &[QueryEvent]) -> Self {
        let mut s = Self::default();
        for e in events {
            s.total += 1;
            *s.per_domain.entry(e.domain.clone()).or_insert(0) += 1;
            if e.blocked {
                s.blocked += 1;
                *s.blocked_per_domain.entry(e.domain.clone()).or_insert(0) += 1;
            }
        }
        s
    }

    /// Aggregate a log of raw queried domains by running each through a
    /// [`RuleSet`] first. This is how the engine and the dashboard meet: feed a
    /// day's lookups, get the day's figures.
    #[must_use]
    pub fn from_queries(rules: &RuleSet, queries: &[&str]) -> Self {
        let events: Vec<QueryEvent> = queries
            .iter()
            .map(|q| {
                let blocked = matches!(rules.decide(q), FilterDecision::Blocked(_));
                QueryEvent::new(*q, blocked)
            })
            .collect();
        Self::from_events(&events)
    }

    /// The share of queries that were blocked, 0.0..=1.0. Zero queries → 0.0.
    // Query counts are tiny relative to f64's 52-bit mantissa in practice.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn blocked_ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.blocked as f64 / self.total as f64
        }
    }

    /// The top `n` most-blocked domains, descending by count then domain name
    /// (so ties are deterministic).
    #[must_use]
    pub fn top_blocked(&self, n: usize) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self
            .blocked_per_domain
            .iter()
            .map(|(d, c)| (d.clone(), *c))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::Rule;

    #[test]
    fn counts_totals_and_blocked() {
        let events = [
            QueryEvent::new("a.example.com", true),
            QueryEvent::new("a.example.com", true),
            QueryEvent::new("b.example.com", false),
            QueryEvent::new("c.example.com", true),
        ];
        let s = Stats::from_events(&events);
        assert_eq!(s.total, 4);
        assert_eq!(s.blocked, 3);
        assert_eq!(s.per_domain.get("a.example.com"), Some(&2));
        assert_eq!(s.blocked_per_domain.get("a.example.com"), Some(&2));
        assert_eq!(s.blocked_per_domain.get("b.example.com"), None);
    }

    #[test]
    fn blocked_ratio_handles_empty_and_full() {
        assert_eq!(Stats::default().blocked_ratio(), 0.0);
        let all_blocked = [QueryEvent::new("x", true), QueryEvent::new("y", true)];
        assert!((Stats::from_events(&all_blocked).blocked_ratio() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn top_blocked_is_sorted_and_truncated() {
        let events = [
            QueryEvent::new("ads.example.com", true),
            QueryEvent::new("ads.example.com", true),
            QueryEvent::new("ads.example.com", true),
            QueryEvent::new("track.example.net", true),
            QueryEvent::new("track.example.net", true),
            QueryEvent::new("beacon.example.org", true),
        ];
        let s = Stats::from_events(&events);
        let top = s.top_blocked(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0], ("ads.example.com".to_string(), 3));
        assert_eq!(top[1], ("track.example.net".to_string(), 2));
    }

    #[test]
    fn from_queries_runs_them_through_the_engine() {
        let mut rs = RuleSet::new();
        rs.add_client_rule(Rule::block_subdomain("ads.example.com"));
        let queries = [
            "ads.example.com",
            "img.ads.example.com",
            "wikipedia.org",
            "news.example.com",
        ];
        let s = Stats::from_queries(&rs, &queries);
        assert_eq!(s.total, 4);
        assert_eq!(s.blocked, 2);
        assert_eq!(s.blocked_per_domain.get("img.ads.example.com"), Some(&1));
    }

    #[test]
    fn empty_log_is_all_zero() {
        let s = Stats::from_events(&[]);
        assert_eq!(s.total, 0);
        assert_eq!(s.blocked, 0);
        assert!(s.per_domain.is_empty());
        assert!(s.top_blocked(5).is_empty());
    }
}
