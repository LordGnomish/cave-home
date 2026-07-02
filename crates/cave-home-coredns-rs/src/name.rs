// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! RFC 1035 domain names.
//!
//! A [`Name`] is a sequence of labels stored most-significant-first (the way it
//! is written: `www.example.com` → `[www, example, com]`); the root is the
//! empty sequence. Each label is 1–63 octets and the whole name is at most 255
//! octets on the wire (RFC 1035 §2.3.4). Case is preserved on construction but
//! ignored for equality, hashing and ordering, exactly as DNS requires for
//! ASCII labels (RFC 4034 §3.1.7 canonical form lowercases A–Z).

use crate::error::{Result, WireError};
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};

/// The RFC 1035 §2.3.4 limit on a single label.
pub const MAX_LABEL: usize = 63;
/// The RFC 1035 §2.3.4 limit on a name's encoded (wire) length.
pub const MAX_NAME_WIRE: usize = 255;

/// A DNS domain name: an ordered list of labels, root = empty.
#[derive(Debug, Clone, Default)]
pub struct Name {
    /// Labels most-significant-first, each 1..=63 octets; empty list = root.
    labels: Vec<Vec<u8>>,
}

impl Name {
    /// The root name (`.`).
    #[must_use]
    pub const fn root() -> Self {
        Self { labels: Vec::new() }
    }

    /// Build a name from raw label octets, validating label and total length.
    ///
    /// # Errors
    /// [`WireError::LabelTooLong`] / [`WireError::NameTooLong`] /
    /// [`WireError::InvalidLabel`] on a violating label or oversize name.
    pub fn from_labels(labels: Vec<Vec<u8>>) -> Result<Self> {
        for label in &labels {
            if label.is_empty() {
                return Err(WireError::InvalidLabel);
            }
            if label.len() > MAX_LABEL {
                return Err(WireError::LabelTooLong { len: label.len() });
            }
        }
        let name = Self { labels };
        let wire = name.len_on_wire();
        if wire > MAX_NAME_WIRE {
            return Err(WireError::NameTooLong { len: wire });
        }
        Ok(name)
    }

    /// Parse a presentation-format name (`foo.bar.com`, with or without a
    /// trailing dot; `.` is the root).
    ///
    /// # Errors
    /// As [`Name::from_labels`], plus [`WireError::InvalidLabel`] for an empty
    /// interior label (e.g. `foo..bar`).
    pub fn parse(s: &str) -> Result<Self> {
        if s == "." || s.is_empty() {
            return Ok(Self::root());
        }
        // A single trailing dot denotes a fully-qualified name; strip it once.
        let trimmed = s.strip_suffix('.').unwrap_or(s);
        let mut labels = Vec::new();
        for part in trimmed.split('.') {
            if part.is_empty() {
                return Err(WireError::InvalidLabel);
            }
            labels.push(part.as_bytes().to_vec());
        }
        Self::from_labels(labels)
    }

    /// Whether this is the root name.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.labels.is_empty()
    }

    /// The number of labels (root = 0).
    #[must_use]
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// The labels, most-significant-first.
    #[must_use]
    pub fn labels(&self) -> &[Vec<u8>] {
        &self.labels
    }

    /// The encoded length: each label contributes `1 + len`, plus the root
    /// terminator octet.
    #[must_use]
    pub fn len_on_wire(&self) -> usize {
        self.labels.iter().map(|l| 1 + l.len()).sum::<usize>() + 1
    }

    /// The name with every ASCII letter lower-cased (RFC 4034 canonical form).
    #[must_use]
    pub fn canonical(&self) -> Self {
        Self {
            labels: self.labels.iter().map(|l| l.to_ascii_lowercase()).collect(),
        }
    }

    /// The name with its left-most label removed, or `None` for the root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.labels.is_empty() {
            None
        } else {
            Some(Self {
                labels: self.labels[1..].to_vec(),
            })
        }
    }

    /// Whether `self` is equal to or below `parent` in the name tree.
    #[must_use]
    pub fn is_subdomain_of(&self, parent: &Self) -> bool {
        if parent.labels.len() > self.labels.len() {
            return false;
        }
        // `parent`'s labels must match `self`'s trailing labels, right-aligned.
        let offset = self.labels.len() - parent.labels.len();
        self.labels[offset..]
            .iter()
            .zip(&parent.labels)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }

    /// RFC 4034 §6.1 canonical ordering: compare labels right-to-left (root
    /// first), each as a case-insensitive octet string; fewer labels sorts
    /// first when the compared labels are equal.
    #[must_use]
    pub fn cmp_canonical(&self, other: &Self) -> Ordering {
        let mut a = self.labels.iter().rev();
        let mut b = other.labels.iter().rev();
        loop {
            match (a.next(), b.next()) {
                (None, None) => return Ordering::Equal,
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(la), Some(lb)) => {
                    let ord = la
                        .iter()
                        .map(u8::to_ascii_lowercase)
                        .cmp(lb.iter().map(u8::to_ascii_lowercase));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
            }
        }
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        self.labels.len() == other.labels.len()
            && self
                .labels
                .iter()
                .zip(&other.labels)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }
}

impl Eq for Name {}

impl Hash for Name {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the case-folded form so equal names hash equally.
        for label in &self.labels {
            for byte in label {
                state.write_u8(byte.to_ascii_lowercase());
            }
            state.write_u8(b'.');
        }
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.labels.is_empty() {
            return f.write_str(".");
        }
        for label in &self.labels {
            for &byte in label {
                // Presentation form escapes non-printable / dot / backslash.
                match byte {
                    b'.' | b'\\' => write!(f, "\\{}", byte as char)?,
                    0x20..=0x7e => write!(f, "{}", byte as char)?,
                    other => write!(f, "\\{other:03}")?,
                }
            }
            f.write_str(".")?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use core::cmp::Ordering;

    #[test]
    fn root_has_no_labels_and_prints_as_dot() {
        let root = Name::root();
        assert!(root.is_root());
        assert_eq!(root.label_count(), 0);
        assert_eq!(root.to_string(), ".");
        assert_eq!(root.len_on_wire(), 1);
    }

    #[test]
    fn parse_splits_labels_and_prints_fqdn() {
        let n = Name::parse("foo.bar.com").unwrap();
        assert_eq!(n.label_count(), 3);
        assert_eq!(n.to_string(), "foo.bar.com.");
        // 1+3 + 1+3 + 1+3 + 1(root) = 13
        assert_eq!(n.len_on_wire(), 13);
    }

    #[test]
    fn trailing_dot_is_equivalent() {
        assert_eq!(
            Name::parse("foo.bar.").unwrap(),
            Name::parse("foo.bar").unwrap()
        );
    }

    #[test]
    fn equality_is_ascii_case_insensitive() {
        assert_eq!(
            Name::parse("FoO.CoM").unwrap(),
            Name::parse("foo.com").unwrap()
        );
    }

    #[test]
    fn parse_rejects_empty_interior_label() {
        assert!(Name::parse("foo..bar").is_err());
    }

    #[test]
    fn parse_rejects_label_over_63_octets() {
        let long = "a".repeat(64);
        assert!(Name::parse(&long).is_err());
    }

    #[test]
    fn parse_rejects_name_over_255_octets() {
        // 4 labels of 63 + dots = 255 presentation, 259 on wire → too long.
        let label = "a".repeat(63);
        let huge = [label.as_str(); 5].join(".");
        assert!(Name::parse(&huge).is_err());
    }

    #[test]
    fn subdomain_relation() {
        let child = Name::parse("a.b.example.com").unwrap();
        let parent = Name::parse("example.com").unwrap();
        let other = Name::parse("example.org").unwrap();
        assert!(child.is_subdomain_of(&parent));
        assert!(parent.is_subdomain_of(&Name::root()));
        assert!(!parent.is_subdomain_of(&child));
        assert!(!child.is_subdomain_of(&other));
        // A name is its own subdomain.
        assert!(parent.is_subdomain_of(&parent));
    }

    #[test]
    fn parent_strips_the_leftmost_label() {
        let n = Name::parse("a.b.c").unwrap();
        assert_eq!(n.parent().unwrap(), Name::parse("b.c").unwrap());
        assert_eq!(Name::parse("c").unwrap().parent().unwrap(), Name::root());
        assert!(Name::root().parent().is_none());
    }

    #[test]
    fn canonical_form_lowercases() {
        let n = Name::parse("WwW.Example.COM").unwrap();
        assert_eq!(n.canonical().to_string(), "www.example.com.");
    }

    #[test]
    fn rfc4034_canonical_ordering() {
        // RFC 4034 §6.1: order label-by-label, right to left, case-insensitive,
        // by octet; fewer labels sorts first when compared labels are equal.
        let a = Name::parse("example.").unwrap();
        let b = Name::parse("a.example.").unwrap();
        let c = Name::parse("z.example.").unwrap();
        assert_eq!(a.cmp_canonical(&b), Ordering::Less);
        assert_eq!(b.cmp_canonical(&c), Ordering::Less);
        assert_eq!(c.cmp_canonical(&a), Ordering::Greater);
        assert_eq!(a.cmp_canonical(&a), Ordering::Equal);
        // Right-to-left: "z.a.example" < "z.example" because the second-from-
        // right label "a" < "(absent)"? No — shorter sorts first when it is a
        // prefix from the right.
        let za = Name::parse("z.a.example.").unwrap();
        assert_eq!(c.cmp_canonical(&za), Ordering::Greater);
        // Case-insensitive: "Z.a.example" sorts with "z.a.example".
        assert_eq!(
            Name::parse("Z.a.EXAMPLE.").unwrap().cmp_canonical(&za),
            Ordering::Equal
        );
    }

    #[test]
    fn hash_is_case_insensitive() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Name::parse("Foo.Bar").unwrap());
        assert!(set.contains(&Name::parse("foo.bar").unwrap()));
    }

    #[test]
    fn from_labels_validates_and_round_trips() {
        let n = Name::from_labels(vec![b"foo".to_vec(), b"bar".to_vec()]).unwrap();
        assert_eq!(n.to_string(), "foo.bar.");
        assert!(Name::from_labels(vec![vec![b'a'; 64]]).is_err());
    }
}
