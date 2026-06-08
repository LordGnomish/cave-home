// SPDX-License-Identifier: Apache-2.0
//! Router model + priority-based selection.
//!
//! A [`Router`] binds a parsed rule to a service, a set of entrypoints, an
//! optional middleware list, and TLS on/off. When several routers match a
//! request, Traefik selects the highest-priority one.
//!
//! Spec basis (public Traefik routers docs):
//! * A router's default priority equals the length of its rule string, so the
//!   most specific (longest) rule wins by default.
//! * An explicit `priority` overrides the default.
//! * Ties are broken deterministically; here we break by router name to keep
//!   selection stable and reproducible.

use crate::request::RequestDescriptor;
use crate::rule::{parse, ParseError, Rule};
use crate::matcher::matches;

/// A configured HTTP router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Router {
    /// Router name (unique within a config).
    pub name: String,
    /// The original rule text (used for the default-priority computation).
    pub rule_text: String,
    /// The parsed rule AST.
    pub rule: Rule,
    /// Entrypoints this router listens on (e.g. `web`, `websecure`).
    pub entrypoints: Vec<String>,
    /// Name of the service requests are forwarded to.
    pub service: String,
    /// Ordered middleware names applied before the service.
    pub middlewares: Vec<String>,
    /// Explicit priority; `None` means "use the rule-length default".
    pub priority: Option<usize>,
    /// Whether TLS termination is enabled for this router.
    pub tls: bool,
}

impl Router {
    /// Build a router from rule text, parsing the rule.
    ///
    /// # Errors
    /// Returns [`ParseError`] if `rule_text` is not a valid rule.
    pub fn new(name: &str, rule_text: &str, service: &str) -> Result<Self, ParseError> {
        let rule = parse(rule_text)?;
        Ok(Self {
            name: name.to_string(),
            rule_text: rule_text.to_string(),
            rule,
            entrypoints: Vec::new(),
            service: service.to_string(),
            middlewares: Vec::new(),
            priority: None,
            tls: false,
        })
    }

    /// Builder: set entrypoints.
    #[must_use]
    pub fn with_entrypoints(mut self, eps: &[&str]) -> Self {
        self.entrypoints = eps.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Builder: set the middleware list.
    #[must_use]
    pub fn with_middlewares(mut self, mws: &[&str]) -> Self {
        self.middlewares = mws.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Builder: set an explicit priority.
    #[must_use]
    pub const fn with_priority(mut self, priority: usize) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Builder: enable TLS.
    #[must_use]
    pub const fn with_tls(mut self, tls: bool) -> Self {
        self.tls = tls;
        self
    }

    /// The effective priority: explicit value, or the rule-length default.
    #[must_use]
    pub fn effective_priority(&self) -> usize {
        self.priority
            .unwrap_or_else(|| Rule::default_priority(&self.rule_text))
    }

    /// Whether this router matches `req`. If `entrypoint` is `Some`, the router
    /// must also be bound to that entrypoint.
    #[must_use]
    pub fn accepts(&self, req: &RequestDescriptor, entrypoint: Option<&str>) -> bool {
        if let Some(ep) = entrypoint
            && !self.entrypoints.is_empty()
            && !self.entrypoints.iter().any(|e| e == ep)
        {
            return false;
        }
        matches(&self.rule, req)
    }
}

/// Select the best matching router for `req` on `entrypoint` from `routers`.
///
/// Selection: among all routers that [`Router::accepts`] the request, pick the
/// highest [`Router::effective_priority`]; ties are broken by router name
/// (lexicographically smallest wins) for deterministic, reproducible routing.
#[must_use]
pub fn select<'a>(
    routers: &'a [Router],
    req: &RequestDescriptor,
    entrypoint: Option<&str>,
) -> Option<&'a Router> {
    routers
        .iter()
        .filter(|r| r.accepts(req, entrypoint))
        .max_by(|a, b| {
            a.effective_priority()
                .cmp(&b.effective_priority())
                // tie-break: smaller name wins -> reverse so max_by picks it
                .then_with(|| b.name.cmp(&a.name))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> RequestDescriptor {
        RequestDescriptor::new("GET", "https", "example.com", "/api/users")
    }

    #[test]
    fn effective_priority_defaults_to_rule_length() {
        let r = Router::new("r", "Host(`a`)", "svc").unwrap();
        assert_eq!(r.effective_priority(), "Host(`a`)".len());
    }

    #[test]
    fn explicit_priority_overrides_default() {
        let r = Router::new("r", "Host(`a`)", "svc").unwrap().with_priority(5000);
        assert_eq!(r.effective_priority(), 5000);
    }

    #[test]
    fn longest_rule_wins_by_default() {
        let broad = Router::new("broad", "PathPrefix(`/`)", "s1").unwrap();
        let specific = Router::new("specific", "Host(`example.com`) && PathPrefix(`/api`)", "s2").unwrap();
        let routers = [broad, specific];
        let chosen = select(&routers, &req(), None).unwrap();
        assert_eq!(chosen.name, "specific");
    }

    #[test]
    fn explicit_priority_beats_longer_rule() {
        let broad = Router::new("broad", "PathPrefix(`/`)", "s1").unwrap().with_priority(10_000);
        let specific = Router::new("specific", "Host(`example.com`) && PathPrefix(`/api`)", "s2").unwrap();
        let routers = [broad, specific];
        let chosen = select(&routers, &req(), None).unwrap();
        assert_eq!(chosen.name, "broad");
    }

    #[test]
    fn tie_broken_by_name_deterministically() {
        // Two routers, identical rule text -> identical priority. Name "aaa" < "zzz".
        let a = Router::new("zzz", "Host(`example.com`)", "s1").unwrap();
        let b = Router::new("aaa", "Host(`example.com`)", "s2").unwrap();
        let routers = [a, b];
        let chosen = select(&routers, &req(), None).unwrap();
        assert_eq!(chosen.name, "aaa");
    }

    #[test]
    fn no_match_returns_none() {
        let r = Router::new("r", "Host(`other.com`)", "svc").unwrap();
        assert!(select(&[r], &req(), None).is_none());
    }

    #[test]
    fn entrypoint_filter_excludes_unbound_router() {
        let r = Router::new("r", "Host(`example.com`)", "svc")
            .unwrap()
            .with_entrypoints(&["websecure"]);
        // wrong entrypoint -> excluded
        assert!(select(std::slice::from_ref(&r), &req(), Some("web")).is_none());
        // right entrypoint -> matches
        assert_eq!(select(&[r], &req(), Some("websecure")).unwrap().name, "r");
    }

    #[test]
    fn empty_entrypoints_matches_any_entrypoint() {
        let r = Router::new("r", "Host(`example.com`)", "svc").unwrap();
        assert!(select(&[r], &req(), Some("web")).is_some());
    }

    #[test]
    fn accepts_respects_rule() {
        let r = Router::new("r", "Method(`POST`)", "svc").unwrap();
        assert!(!r.accepts(&req(), None));
    }
}
