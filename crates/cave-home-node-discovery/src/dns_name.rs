//! DNS domain name <-> wire round-trip, built on [`crate::label`].
//!
//! A [`DnsName`] is an ordered sequence of labels (e.g.
//! `["_cavehome", "_tcp", "local"]`). On the wire it is each label in its
//! length-prefixed form, terminated by the zero-length root label. Names are
//! compared case-insensitively (DNS is ASCII-case-insensitive, RFC 4343).

use crate::label::{decode_label, encode_label, LabelError, MAX_LABEL_LEN};

/// The maximum length of an encoded name on the wire, including the root byte
/// (RFC 1035 §2.3.4).
pub const MAX_NAME_WIRE_LEN: usize = 255;

/// A DNS domain name as an ordered list of labels (no trailing root label
/// stored; it is implied and emitted on encode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsName {
    labels: Vec<String>,
}

/// Why a [`DnsName`] could not be built, encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    /// A name with no labels (other than the root) is not valid here.
    Empty,
    /// A label was malformed (empty, too long, …); carries the cause.
    Label(LabelError),
    /// The fully encoded name would exceed [`MAX_NAME_WIRE_LEN`].
    TooLong,
    /// Decoding ran off the end of the wire without hitting the root label.
    Unterminated,
    /// A label contained a non-ASCII octet (mDNS service names are ASCII).
    NotAscii,
}

impl core::fmt::Display for NameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("DNS name has no labels"),
            Self::Label(e) => write!(f, "DNS name label invalid: {e}"),
            Self::TooLong => write!(f, "DNS name exceeds {MAX_NAME_WIRE_LEN} wire octets"),
            Self::Unterminated => f.write_str("DNS name wire bytes are unterminated"),
            Self::NotAscii => f.write_str("DNS name label is not ASCII"),
        }
    }
}

impl std::error::Error for NameError {}

impl From<LabelError> for NameError {
    fn from(e: LabelError) -> Self {
        Self::Label(e)
    }
}

impl DnsName {
    /// Build a name from its labels (each WITHOUT a length prefix or dot).
    ///
    /// # Errors
    /// - [`NameError::Empty`] if `labels` is empty.
    /// - [`NameError::Label`] if any label is empty or over
    ///   [`MAX_LABEL_LEN`] octets.
    /// - [`NameError::NotAscii`] if any label has a non-ASCII octet.
    pub fn new<I, S>(labels: I) -> Result<Self, NameError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let labels: Vec<String> = labels.into_iter().map(Into::into).collect();
        if labels.is_empty() {
            return Err(NameError::Empty);
        }
        for l in &labels {
            if l.is_empty() {
                return Err(NameError::Label(LabelError::Empty));
            }
            if l.len() > MAX_LABEL_LEN {
                return Err(NameError::Label(LabelError::TooLong(l.len())));
            }
            if !l.is_ascii() {
                return Err(NameError::NotAscii);
            }
        }
        Ok(Self { labels })
    }

    /// Parse a dotted name (e.g. `"kitchen.local"`).
    ///
    /// # Errors
    /// Same conditions as [`DnsName::new`]; an empty string or a string of
    /// only dots yields [`NameError::Empty`].
    pub fn parse(dotted: &str) -> Result<Self, NameError> {
        // Tolerate a single trailing dot (the fully-qualified form).
        let trimmed = dotted.strip_suffix('.').unwrap_or(dotted);
        if trimmed.is_empty() {
            return Err(NameError::Empty);
        }
        Self::new(trimmed.split('.').map(str::to_owned))
    }

    /// The labels, in order, without the implied root.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Render as a dotted string (no trailing dot), e.g. `"_cavehome._tcp.local"`.
    #[must_use]
    pub fn to_dotted(&self) -> String {
        self.labels.join(".")
    }

    /// Encode to the length-prefixed wire form, terminated by the root label.
    ///
    /// # Errors
    /// - [`NameError::Label`] if a label fails to encode.
    /// - [`NameError::TooLong`] if the total exceeds [`MAX_NAME_WIRE_LEN`].
    pub fn encode(&self) -> Result<Vec<u8>, NameError> {
        let mut out = Vec::new();
        for label in &self.labels {
            encode_label(label, &mut out)?;
        }
        out.push(0); // root terminator
        if out.len() > MAX_NAME_WIRE_LEN {
            return Err(NameError::TooLong);
        }
        Ok(out)
    }

    /// Decode a name starting at `wire[pos]`, returning the name and the
    /// position immediately after its root terminator.
    ///
    /// # Errors
    /// - [`NameError::Unterminated`] if the bytes end before the root label.
    /// - [`NameError::Label`] for a malformed label (including a deferred
    ///   compression pointer).
    /// - [`NameError::NotAscii`] if a decoded label is not valid ASCII.
    pub fn decode(wire: &[u8], pos: usize) -> Result<(Self, usize), NameError> {
        let mut labels = Vec::new();
        let mut cursor = pos;
        loop {
            let (bytes, next) = decode_label(wire, cursor)?;
            cursor = next;
            if bytes.is_empty() {
                // Root label: end of name.
                break;
            }
            if !bytes.is_ascii() {
                return Err(NameError::NotAscii);
            }
            // ASCII-validated above, so from_utf8 cannot fail.
            match String::from_utf8(bytes) {
                Ok(s) => labels.push(s),
                Err(_) => return Err(NameError::NotAscii),
            }
        }
        if labels.is_empty() {
            return Err(NameError::Empty);
        }
        Ok((Self { labels }, cursor))
    }

    /// Case-insensitive name equality (DNS labels are ASCII-case-insensitive).
    #[must_use]
    pub fn eq_ignore_case(&self, other: &Self) -> bool {
        self.labels.len() == other.labels.len()
            && self
                .labels
                .iter()
                .zip(&other.labels)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }
}

impl core::fmt::Display for DnsName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_dotted())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dotted_name() {
        let n = DnsName::parse("_cavehome._tcp.local").expect("parses");
        assert_eq!(n.labels(), &["_cavehome", "_tcp", "local"]);
        assert_eq!(n.to_dotted(), "_cavehome._tcp.local");
    }

    #[test]
    fn parse_tolerates_trailing_dot() {
        let n = DnsName::parse("kitchen.local.").expect("parses");
        assert_eq!(n.to_dotted(), "kitchen.local");
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(DnsName::parse(""), Err(NameError::Empty));
        assert_eq!(DnsName::parse("."), Err(NameError::Empty));
    }

    #[test]
    fn new_rejects_empty_label() {
        assert_eq!(
            DnsName::new(["a", "", "b"]),
            Err(NameError::Label(LabelError::Empty))
        );
    }

    #[test]
    fn new_rejects_non_ascii() {
        assert_eq!(DnsName::new(["café"]), Err(NameError::NotAscii));
    }

    #[test]
    fn wire_round_trip_service_name() {
        let n = DnsName::parse("_cavehome._tcp.local").expect("parses");
        let wire = n.encode().expect("encodes");
        // 1+9 + 1+4 + 1+5 + 1(root) = 22 octets.
        assert_eq!(wire.len(), 22);
        assert_eq!(*wire.last().expect("non-empty"), 0u8);
        let (back, consumed) = DnsName::decode(&wire, 0).expect("decodes");
        assert_eq!(back, n);
        assert_eq!(consumed, wire.len());
    }

    #[test]
    fn wire_round_trip_instance_name() {
        let n = DnsName::parse("hub-kitchen._cavehome._tcp.local").expect("parses");
        let wire = n.encode().expect("encodes");
        let (back, _) = DnsName::decode(&wire, 0).expect("decodes");
        assert!(back.eq_ignore_case(&n));
    }

    #[test]
    fn decode_unterminated_errors() {
        // A label declared but no root terminator before the bytes run out.
        let wire = vec![5, b'l', b'o', b'c', b'a', b'l'];
        assert!(matches!(
            DnsName::decode(&wire, 0),
            Err(NameError::Label(LabelError::Truncated))
        ));
    }

    #[test]
    fn case_insensitive_equality() {
        let a = DnsName::parse("Kitchen.Local").expect("parses");
        let b = DnsName::parse("kitchen.local").expect("parses");
        assert!(a.eq_ignore_case(&b));
        // Plain Eq is case-sensitive (stores raw labels).
        assert_ne!(a, b);
    }

    #[test]
    fn encode_rejects_overlong_name() {
        // Many maximal labels overflow the 255-octet wire cap.
        let label = "a".repeat(MAX_LABEL_LEN);
        let labels: Vec<String> = (0..5).map(|_| label.clone()).collect();
        let n = DnsName::new(labels).expect("labels individually valid");
        assert_eq!(n.encode(), Err(NameError::TooLong));
    }
}
