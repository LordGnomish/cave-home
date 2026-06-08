//! Topic — the pub/sub channel a notification is published to.
//!
//! A topic is a short, validated name (think "kitchen-leak" or "front-door").
//! Subscribers register interest in a topic (see [`crate::route`]); publishers
//! send to one. The validation rules — non-empty, length-bounded, a restricted
//! charset — are first-party (ADR-021), chosen so a topic is always safe to use
//! as a URL path segment, a file name and a log key without escaping.

/// The longest a topic name may be, in characters.
pub const MAX_LEN: usize = 64;

/// A validated pub/sub channel name.
///
/// Construct one with [`Topic::new`]; an invalid name is rejected up front so
/// nothing downstream has to defend against an empty or unsafe channel.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Topic(String);

/// Why a [`Topic`] name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicError {
    /// The name had no characters.
    Empty,
    /// The name was longer than [`MAX_LEN`] characters.
    TooLong,
    /// The name contained a character outside `a-z A-Z 0-9 _ -`.
    BadChar,
}

impl core::fmt::Display for TopicError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("channel name is empty"),
            Self::TooLong => f.write_str("channel name is too long"),
            Self::BadChar => {
                f.write_str("channel name has a character that is not a letter, digit, '_' or '-'")
            }
        }
    }
}

impl std::error::Error for TopicError {}

impl Topic {
    /// Validate and build a topic name.
    ///
    /// The name must be non-empty, at most [`MAX_LEN`] characters, and contain
    /// only ASCII letters, digits, `_` or `-`.
    ///
    /// # Errors
    /// Returns [`TopicError`] describing the first rule the name breaks.
    pub fn new(name: impl Into<String>) -> Result<Self, TopicError> {
        let name = name.into();
        if name.is_empty() {
            return Err(TopicError::Empty);
        }
        if name.chars().count() > MAX_LEN {
            return Err(TopicError::TooLong);
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(TopicError::BadChar);
        }
        Ok(Self(name))
    }

    /// The validated name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for Topic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_names() {
        assert_eq!(Topic::new("kitchen-leak").unwrap().as_str(), "kitchen-leak");
        assert_eq!(Topic::new("front_door").unwrap().as_str(), "front_door");
        assert_eq!(Topic::new("Zone2").unwrap().as_str(), "Zone2");
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(Topic::new(""), Err(TopicError::Empty));
    }

    #[test]
    fn rejects_too_long() {
        let ok = "a".repeat(MAX_LEN);
        assert!(Topic::new(ok).is_ok());
        let bad = "a".repeat(MAX_LEN + 1);
        assert_eq!(Topic::new(bad), Err(TopicError::TooLong));
    }

    #[test]
    fn rejects_bad_chars() {
        assert_eq!(Topic::new("has space"), Err(TopicError::BadChar));
        assert_eq!(Topic::new("slash/path"), Err(TopicError::BadChar));
        assert_eq!(Topic::new("emoji😀"), Err(TopicError::BadChar));
        assert_eq!(Topic::new("dot.name"), Err(TopicError::BadChar));
    }

    #[test]
    fn equal_names_are_equal_topics() {
        assert_eq!(Topic::new("a").unwrap(), Topic::new("a").unwrap());
        assert_ne!(Topic::new("a").unwrap(), Topic::new("b").unwrap());
    }
}
