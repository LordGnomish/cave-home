// SPDX-License-Identifier: Apache-2.0
//! Label and field selector parsing + matching.
//!
//! Behavioural reference: Kubernetes docs "Labels and Selectors"
//! (set-based + equality-based requirements) and the documented field-selector
//! syntax (`key=value`, `key!=value`, comma = AND). Clean-room reimplementation
//! of the documented selector grammar.

use std::collections::BTreeMap;

use crate::status::Status;

/// One requirement within a label selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Requirement {
    /// `key=value` / `key==value`.
    Equals(String, String),
    /// `key!=value`.
    NotEquals(String, String),
    /// `key in (a, b, c)`.
    In(String, Vec<String>),
    /// `key notin (a, b, c)`.
    NotIn(String, Vec<String>),
    /// `key` — the key must exist.
    Exists(String),
    /// `!key` — the key must not exist.
    NotExists(String),
}

impl Requirement {
    /// Test a single requirement against a label map.
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        match self {
            Requirement::Equals(k, v) => labels.get(k).map(|x| x == v).unwrap_or(false),
            // k8s: `key!=value` is true when key is absent OR has a different value.
            Requirement::NotEquals(k, v) => labels.get(k).map(|x| x != v).unwrap_or(true),
            Requirement::In(k, set) => labels.get(k).map(|x| set.contains(x)).unwrap_or(false),
            Requirement::NotIn(k, set) => labels.get(k).map(|x| !set.contains(x)).unwrap_or(true),
            Requirement::Exists(k) => labels.contains_key(k),
            Requirement::NotExists(k) => !labels.contains_key(k),
        }
    }
}

/// A conjunction (AND) of requirements. An empty selector matches everything.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LabelSelector {
    /// All requirements must hold (logical AND).
    pub requirements: Vec<Requirement>,
}

impl LabelSelector {
    /// The everything-selector.
    #[must_use]
    pub fn everything() -> Self {
        Self::default()
    }

    /// True if every requirement matches the given labels.
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.requirements.iter().all(|r| r.matches(labels))
    }

    /// Parse a label selector string (`key=val,key2 in (a,b),!key3,key4`).
    ///
    /// # Errors
    /// Returns a `BadRequest` [`Status`] on malformed syntax.
    pub fn parse(input: &str) -> Result<Self, Status> {
        let mut requirements = Vec::new();
        for raw in split_top_level(input) {
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            requirements.push(parse_requirement(part)?);
        }
        Ok(Self { requirements })
    }
}

/// Split a selector on commas that are not inside parentheses (set values).
fn split_top_level(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut cur = String::new();
    for c in input.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_set(rest: &str) -> Result<Vec<String>, Status> {
    let inner = rest
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| Status::bad_request(format!("expected (..) set, got {rest:?}")))?;
    Ok(inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

fn parse_requirement(part: &str) -> Result<Requirement, Status> {
    // Set-based: `key in (...)` / `key notin (...)`.
    if let Some((k, rest)) = split_keyword(part, " in ") {
        return Ok(Requirement::In(k, parse_set(rest)?));
    }
    if let Some((k, rest)) = split_keyword(part, " notin ") {
        return Ok(Requirement::NotIn(k, parse_set(rest)?));
    }
    // Equality-based — check `!=`/`==` before single `=`.
    if let Some((k, v)) = part.split_once("!=") {
        return Ok(Requirement::NotEquals(k.trim().to_string(), v.trim().to_string()));
    }
    if let Some((k, v)) = part.split_once("==") {
        return Ok(Requirement::Equals(k.trim().to_string(), v.trim().to_string()));
    }
    if let Some((k, v)) = part.split_once('=') {
        return Ok(Requirement::Equals(k.trim().to_string(), v.trim().to_string()));
    }
    // Existence.
    if let Some(k) = part.strip_prefix('!') {
        let key = k.trim();
        if key.is_empty() {
            return Err(Status::bad_request("`!` requires a key"));
        }
        return Ok(Requirement::NotExists(key.to_string()));
    }
    let key = part.trim();
    if key.contains(char::is_whitespace) {
        return Err(Status::bad_request(format!("malformed requirement {part:?}")));
    }
    Ok(Requirement::Exists(key.to_string()))
}

/// Split `part` on the first case-sensitive occurrence of ` in ` / ` notin `.
fn split_keyword<'a>(part: &'a str, kw: &str) -> Option<(String, &'a str)> {
    part.find(kw).map(|i| (part[..i].trim().to_string(), &part[i + kw.len()..]))
}

/// A field selector: a conjunction of `field=value` / `field!=value` over the
/// object's value tree (dotted paths against `metadata`/`spec`/`status`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FieldSelector {
    /// `(field-path, value, negated)` triples, AND-combined.
    pub requirements: Vec<(String, String, bool)>,
}

impl FieldSelector {
    /// The everything-selector.
    #[must_use]
    pub fn everything() -> Self {
        Self::default()
    }

    /// Parse a field selector (`metadata.namespace=default,status.phase!=Running`).
    ///
    /// # Errors
    /// Returns a `BadRequest` [`Status`] on malformed syntax.
    pub fn parse(input: &str) -> Result<Self, Status> {
        let mut requirements = Vec::new();
        for raw in input.split(',') {
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once("!=") {
                requirements.push((k.trim().to_string(), v.trim().to_string(), true));
            } else if let Some((k, v)) = part.split_once('=') {
                requirements.push((k.trim().to_string(), v.trim().to_string(), false));
            } else {
                return Err(Status::bad_request(format!(
                    "field selector requirement must be key=value or key!=value, got {part:?}"
                )));
            }
        }
        Ok(Self { requirements })
    }

    /// Match against an object value tree. A field reads the dotted path; an
    /// absent field is treated as the empty string (matching k8s behaviour for
    /// the common `metadata.*` selectors).
    #[must_use]
    pub fn matches(&self, object: &crate::json::Value) -> bool {
        self.requirements.iter().all(|(path, want, negated)| {
            let actual = object.pointer(path).and_then(|v| v.as_str()).unwrap_or("");
            if *negated {
                actual != want
            } else {
                actual == want
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::obj;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
    }

    #[test]
    fn parse_equality_and_inequality() {
        let s = LabelSelector::parse("app=nginx,tier!=db").expect("parse");
        assert_eq!(
            s.requirements,
            vec![
                Requirement::Equals("app".into(), "nginx".into()),
                Requirement::NotEquals("tier".into(), "db".into()),
            ]
        );
    }

    #[test]
    fn parse_set_based_in() {
        let s = LabelSelector::parse("env in (prod, staging)").expect("parse");
        assert_eq!(
            s.requirements,
            vec![Requirement::In("env".into(), vec!["prod".into(), "staging".into()])]
        );
    }

    #[test]
    fn parse_exists_and_not_exists() {
        let s = LabelSelector::parse("ready,!broken").expect("parse");
        assert_eq!(
            s.requirements,
            vec![Requirement::Exists("ready".into()), Requirement::NotExists("broken".into())]
        );
    }

    #[test]
    fn equals_matches() {
        let s = LabelSelector::parse("app=nginx").expect("parse");
        assert!(s.matches(&labels(&[("app", "nginx")])));
        assert!(!s.matches(&labels(&[("app", "redis")])));
        assert!(!s.matches(&labels(&[])));
    }

    #[test]
    fn not_equals_matches_when_absent() {
        let s = LabelSelector::parse("tier!=db").expect("parse");
        assert!(s.matches(&labels(&[])));
        assert!(s.matches(&labels(&[("tier", "web")])));
        assert!(!s.matches(&labels(&[("tier", "db")])));
    }

    #[test]
    fn in_and_notin_match() {
        let in_sel = LabelSelector::parse("env in (prod,staging)").expect("parse");
        assert!(in_sel.matches(&labels(&[("env", "prod")])));
        assert!(!in_sel.matches(&labels(&[("env", "dev")])));
        let notin = LabelSelector::parse("env notin (prod,staging)").expect("parse");
        assert!(notin.matches(&labels(&[("env", "dev")])));
        assert!(notin.matches(&labels(&[])));
        assert!(!notin.matches(&labels(&[("env", "prod")])));
    }

    #[test]
    fn exists_matches() {
        let s = LabelSelector::parse("ready,!broken").expect("parse");
        assert!(s.matches(&labels(&[("ready", "true")])));
        assert!(!s.matches(&labels(&[("ready", "true"), ("broken", "x")])));
    }

    #[test]
    fn conjunction_requires_all() {
        let s = LabelSelector::parse("app=nginx,env in (prod),!debug").expect("parse");
        assert!(s.matches(&labels(&[("app", "nginx"), ("env", "prod")])));
        assert!(!s.matches(&labels(&[("app", "nginx"), ("env", "dev")])));
    }

    #[test]
    fn empty_selector_matches_everything() {
        let s = LabelSelector::everything();
        assert!(s.matches(&labels(&[])));
        assert!(s.matches(&labels(&[("a", "b")])));
    }

    #[test]
    fn field_selector_matches_dotted_path() {
        let object = obj([(
            "metadata",
            obj([("namespace", crate::json::Value::from("default"))]),
        )]);
        let s = FieldSelector::parse("metadata.namespace=default").expect("parse");
        assert!(s.matches(&object));
        let s2 = FieldSelector::parse("metadata.namespace!=default").expect("parse");
        assert!(!s2.matches(&object));
    }

    #[test]
    fn field_selector_rejects_bare_key() {
        assert!(FieldSelector::parse("metadata.name").is_err());
    }
}
