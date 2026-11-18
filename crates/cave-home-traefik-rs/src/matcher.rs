// SPDX-License-Identifier: Apache-2.0
//! Routing-rule evaluator: decide whether a [`RequestDescriptor`] satisfies a
//! parsed [`Rule`].
//!
//! Spec basis (public Traefik HTTP-routers docs):
//! * `Host` matches the request host, case-insensitively.
//! * `Path` matches the path exactly.
//! * `PathPrefix` matches when the path starts with the prefix.
//! * `Header(name, value)` matches when the named header equals `value`.
//! * `Method` matches the request method, case-insensitively.
//! * Multiple comma-separated arguments to one matcher OR together.

use crate::request::RequestDescriptor;
use crate::rule::Rule;

/// Evaluate `rule` against `req`, returning whether the request matches.
#[must_use]
pub fn matches(rule: &Rule, req: &RequestDescriptor) -> bool {
    match rule {
        Rule::Host(hosts) => hosts.iter().any(|h| h == &req.host),
        Rule::Path(paths) => paths.iter().any(|p| p == &req.path),
        Rule::PathPrefix(prefixes) => prefixes.iter().any(|p| path_has_prefix(&req.path, p)),
        Rule::Header { name, value } => req.header(name) == Some(value.as_str()),
        Rule::Method(methods) => {
            let m = req.method.to_ascii_uppercase();
            methods.iter().any(|x| x == &m)
        }
        Rule::And(a, b) => matches(a, req) && matches(b, req),
        Rule::Or(a, b) => matches(a, req) || matches(b, req),
        Rule::Not(inner) => !matches(inner, req),
    }
}

/// Prefix match on path *segments*: `/api` matches `/api` and `/api/x` but not
/// `/apixyz`. This mirrors Traefik's `PathPrefix`, which is segment-aware.
fn path_has_prefix(path: &str, prefix: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }
    // Exact match, or the next char after the prefix is a `/` boundary, or the
    // prefix itself already ends in `/`.
    let rest = &path[prefix.len()..];
    rest.is_empty() || rest.starts_with('/') || prefix.ends_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::{ParseError, parse};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn req() -> RequestDescriptor {
        RequestDescriptor::new("GET", "https", "example.com", "/api/users")
            .with_header("X-Env", "prod")
    }

    fn matches_str(rule: &str, req: &RequestDescriptor) -> Result<bool, ParseError> {
        Ok(matches(&parse(rule)?, req))
    }

    #[test]
    fn host_matches_case_insensitively() -> TestResult {
        assert!(matches_str("Host(`EXAMPLE.com`)", &req())?);
        assert!(!matches_str("Host(`other.com`)", &req())?);
        Ok(())
    }

    #[test]
    fn host_or_list() -> TestResult {
        assert!(matches_str("Host(`a.com`, `example.com`)", &req())?);
        Ok(())
    }

    #[test]
    fn path_is_exact() -> TestResult {
        assert!(matches_str("Path(`/api/users`)", &req())?);
        assert!(!matches_str("Path(`/api`)", &req())?);
        Ok(())
    }

    #[test]
    fn pathprefix_is_segment_aware() -> TestResult {
        assert!(matches_str("PathPrefix(`/api`)", &req())?);
        assert!(matches_str("PathPrefix(`/api/users`)", &req())?);
        // segment boundary: /ap should NOT match /api/users
        assert!(!matches_str("PathPrefix(`/ap`)", &req())?);
        Ok(())
    }

    #[test]
    fn pathprefix_root_matches_everything() -> TestResult {
        assert!(matches_str("PathPrefix(`/`)", &req())?);
        Ok(())
    }

    #[test]
    fn header_exact_value() -> TestResult {
        assert!(matches_str("Header(`X-Env`, `prod`)", &req())?);
        assert!(!matches_str("Header(`X-Env`, `dev`)", &req())?);
        assert!(!matches_str("Header(`X-Missing`, `prod`)", &req())?);
        Ok(())
    }

    #[test]
    fn method_matches_case_insensitively() -> TestResult {
        assert!(matches_str("Method(`get`)", &req())?);
        assert!(!matches_str("Method(`post`)", &req())?);
        Ok(())
    }

    #[test]
    fn and_requires_both() -> TestResult {
        assert!(matches_str(
            "Host(`example.com`) && PathPrefix(`/api`)",
            &req()
        )?);
        assert!(!matches_str(
            "Host(`example.com`) && PathPrefix(`/nope`)",
            &req()
        )?);
        Ok(())
    }

    #[test]
    fn or_requires_either() -> TestResult {
        assert!(matches_str(
            "Host(`nope.com`) || PathPrefix(`/api`)",
            &req()
        )?);
        assert!(!matches_str(
            "Host(`nope.com`) || PathPrefix(`/nope`)",
            &req()
        )?);
        Ok(())
    }

    #[test]
    fn not_negates() -> TestResult {
        assert!(matches_str("!Host(`nope.com`)", &req())?);
        assert!(!matches_str("!Host(`example.com`)", &req())?);
        Ok(())
    }

    #[test]
    fn precedence_and_over_or_evaluates_correctly() -> TestResult {
        // Host(nope) || Host(example) && Path(/api/users)
        // == Host(nope) || (Host(example) && Path(/api/users)) -> true
        assert!(matches_str(
            "Host(`nope.com`) || Host(`example.com`) && Path(`/api/users`)",
            &req()
        )?);
        // With the AND branch failing on path, only the (false) OR-left remains.
        assert!(!matches_str(
            "Host(`nope.com`) || Host(`example.com`) && Path(`/wrong`)",
            &req()
        )?);
        Ok(())
    }

    #[test]
    fn parentheses_change_grouping_result() -> TestResult {
        // (Host(nope) || Host(example)) && Path(/wrong) -> false
        assert!(!matches_str(
            "(Host(`nope.com`) || Host(`example.com`)) && Path(`/wrong`)",
            &req()
        )?);
        // (Host(nope) || Host(example)) && Path(/api/users) -> true
        assert!(matches_str(
            "(Host(`nope.com`) || Host(`example.com`)) && Path(`/api/users`)",
            &req()
        )?);
        Ok(())
    }

    #[test]
    fn complex_nested_rule() -> TestResult {
        let rule =
            "Host(`example.com`) && (PathPrefix(`/api`) || PathPrefix(`/v2`)) && !Method(`DELETE`)";
        assert!(matches_str(rule, &req())?);
        Ok(())
    }
}
