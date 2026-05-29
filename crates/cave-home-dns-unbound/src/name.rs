//! Domain-name model: validated, normalised [`DnsName`].
//!
//! Implemented first-party from the *public* DNS naming rules
//! (RFC 1035 §2.3.1 preferred name syntax / RFC 1123 §2.1 host-name relaxation):
//! labels are 1–63 octets, a full name is ≤ 253 octets, the permitted label
//! characters are letters, digits and the hyphen (a hyphen may not lead or
//! trail a label). Unbound's BSD source was not copied; this is the documented
//! behaviour expressed in Rust.
//!
//! Normalisation, the part the resolution core relies on, is: lower-case the
//! ASCII letters (DNS is case-insensitive for matching) and store the name in a
//! single canonical form. A canonical [`DnsName`] keeps no trailing dot of its
//! own — the root is the empty name — but [`DnsName::to_fqdn`] renders the
//! familiar trailing-dot fully-qualified form on demand.

/// Why a candidate name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// A label was empty (e.g. `a..b`) — only the single root name may be empty.
    EmptyLabel,
    /// A label exceeded the 63-octet limit.
    LabelTooLong,
    /// The whole name exceeded the 253-octet limit.
    NameTooLong,
    /// A label contained a character outside `[A-Za-z0-9-]`.
    BadCharacter,
    /// A label started or ended with a hyphen.
    HyphenAtEdge,
}

/// A validated, normalised domain name.
///
/// Stored lower-case, without a trailing dot. The root is represented by the
/// empty name (no labels).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DnsName {
    /// Canonical form: lower-case labels joined by `.`, no trailing dot.
    /// Empty string == the DNS root.
    canonical: String,
}

const MAX_LABEL: usize = 63;
const MAX_NAME: usize = 253;

impl DnsName {
    /// Parse and normalise a name from human input.
    ///
    /// Accepts an optional trailing dot (`example.com.`), upper-case letters,
    /// and the bare root (`""` or `"."`). Returns the canonical lower-case,
    /// dot-trimmed form.
    ///
    /// # Errors
    /// Returns a [`NameError`] when a label or the whole name breaks the
    /// RFC 1035 / RFC 1123 length or character rules.
    pub fn parse(input: &str) -> Result<Self, NameError> {
        // The lone root, in either spelling.
        if input.is_empty() || input == "." {
            return Ok(Self {
                canonical: String::new(),
            });
        }
        // A single trailing dot is the FQDN marker and is stripped; any other
        // empty label (leading or doubled dot) is an error.
        let trimmed = input.strip_suffix('.').unwrap_or(input);
        if trimmed.is_empty() {
            return Err(NameError::EmptyLabel);
        }
        if trimmed.len() > MAX_NAME {
            return Err(NameError::NameTooLong);
        }

        let mut canonical = String::with_capacity(trimmed.len());
        for (i, label) in trimmed.split('.').enumerate() {
            Self::validate_label(label)?;
            if i > 0 {
                canonical.push('.');
            }
            for b in label.bytes() {
                canonical.push(b.to_ascii_lowercase() as char);
            }
        }
        Ok(Self { canonical })
    }

    fn validate_label(label: &str) -> Result<(), NameError> {
        if label.is_empty() {
            return Err(NameError::EmptyLabel);
        }
        if label.len() > MAX_LABEL {
            return Err(NameError::LabelTooLong);
        }
        let bytes = label.as_bytes();
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return Err(NameError::HyphenAtEdge);
        }
        for &b in bytes {
            let ok = b.is_ascii_alphanumeric() || b == b'-';
            if !ok {
                return Err(NameError::BadCharacter);
            }
        }
        Ok(())
    }

    /// The canonical form: lower-case, no trailing dot. The root is `""`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// `true` for the DNS root (the empty name).
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.canonical.is_empty()
    }

    /// Render the fully-qualified form with the trailing dot (`example.com.`).
    /// The root renders as `.`.
    #[must_use]
    pub fn to_fqdn(&self) -> String {
        if self.canonical.is_empty() {
            ".".to_string()
        } else {
            format!("{}.", self.canonical)
        }
    }

    /// The labels, most specific first (`www`, `example`, `com`). Empty for the
    /// root.
    #[must_use]
    pub fn labels(&self) -> Vec<&str> {
        if self.canonical.is_empty() {
            Vec::new()
        } else {
            self.canonical.split('.').collect()
        }
    }

    /// Is `self` equal to, or a subdomain of, `zone`?
    ///
    /// `www.example.com` is within `example.com` and within the root, but
    /// `notexample.com` is **not** within `example.com`. The root contains
    /// every name.
    #[must_use]
    pub fn is_within(&self, zone: &Self) -> bool {
        if zone.is_root() {
            return true;
        }
        if self.canonical == zone.canonical {
            return true;
        }
        // Must end with `.zone` (label-aligned), not merely a string suffix.
        self.canonical.len() > zone.canonical.len()
            && self.canonical.ends_with(&zone.canonical)
            && self.canonical.as_bytes()[self.canonical.len() - zone.canonical.len() - 1] == b'.'
    }

    /// Number of labels the name carries below the root. Used to find the most
    /// specific (longest-suffix) matching zone.
    #[must_use]
    pub fn label_count(&self) -> usize {
        if self.canonical.is_empty() {
            0
        } else {
            self.canonical.bytes().filter(|&b| b == b'.').count() + 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalises_case_and_trailing_dot() {
        let n = DnsName::parse("WWW.Example.COM.").expect("valid");
        assert_eq!(n.as_str(), "www.example.com");
        assert_eq!(n.to_fqdn(), "www.example.com.");
    }

    #[test]
    fn root_in_either_spelling() {
        assert!(DnsName::parse("").expect("root").is_root());
        assert!(DnsName::parse(".").expect("root").is_root());
        assert_eq!(DnsName::parse(".").expect("root").to_fqdn(), ".");
        assert_eq!(DnsName::parse("").expect("root").label_count(), 0);
    }

    #[test]
    fn rejects_empty_doubled_and_leading_labels() {
        assert_eq!(DnsName::parse("a..b"), Err(NameError::EmptyLabel));
        assert_eq!(DnsName::parse(".example.com"), Err(NameError::EmptyLabel));
    }

    #[test]
    fn rejects_bad_characters_and_hyphen_edges() {
        assert_eq!(DnsName::parse("ex ample.com"), Err(NameError::BadCharacter));
        assert_eq!(DnsName::parse("under_score.com"), Err(NameError::BadCharacter));
        assert_eq!(DnsName::parse("-lead.com"), Err(NameError::HyphenAtEdge));
        assert_eq!(DnsName::parse("trail-.com"), Err(NameError::HyphenAtEdge));
        // A hyphen in the middle is fine.
        assert_eq!(
            DnsName::parse("my-printer.local").expect("ok").as_str(),
            "my-printer.local"
        );
    }

    #[test]
    fn enforces_label_and_name_length_limits() {
        let long_label = "a".repeat(64);
        assert_eq!(
            DnsName::parse(&format!("{long_label}.com")),
            Err(NameError::LabelTooLong)
        );
        let max_label = "a".repeat(63);
        assert!(DnsName::parse(&format!("{max_label}.com")).is_ok());

        // Build a name longer than 253 octets out of legal labels.
        let huge = std::iter::repeat_n("abcdefgh", 40)
            .collect::<Vec<_>>()
            .join(".");
        assert_eq!(DnsName::parse(&huge), Err(NameError::NameTooLong));
    }

    #[test]
    fn within_is_label_aligned() {
        let www = DnsName::parse("www.example.com").expect("ok");
        let zone = DnsName::parse("example.com").expect("ok");
        let other = DnsName::parse("notexample.com").expect("ok");
        let root = DnsName::parse(".").expect("ok");
        assert!(www.is_within(&zone));
        assert!(zone.is_within(&zone));
        assert!(!other.is_within(&zone));
        assert!(www.is_within(&root), "root contains every name");
    }

    #[test]
    fn label_count_counts_labels() {
        assert_eq!(DnsName::parse("a").expect("ok").label_count(), 1);
        assert_eq!(DnsName::parse("a.b.c").expect("ok").label_count(), 3);
    }
}
