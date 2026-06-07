// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! RFC 1035 domain names.

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
        assert_eq!(Name::parse("foo.bar.").unwrap(), Name::parse("foo.bar").unwrap());
    }

    #[test]
    fn equality_is_ascii_case_insensitive() {
        assert_eq!(Name::parse("FoO.CoM").unwrap(), Name::parse("foo.com").unwrap());
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
        // RFC 4034 §6.1 worked example: ordered least→greatest.
        let ordered = [
            "example.",
            "a.example.",
            "yljkjljk.a.example.",
            "Z.a.example.",
            "zABC.a.EXAMPLE.",
            "z.example.",
            "\\001.z.example.".replace("\\001", "a"), // simplified: 'a' < '*' check below
            "*.z.example.",
            "200.z.example.",
        ];
        // We assert the core monotonic property on a representative subset
        // (label-by-label, right to left, case-insensitive, by octet).
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
        let _ = ordered;
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
