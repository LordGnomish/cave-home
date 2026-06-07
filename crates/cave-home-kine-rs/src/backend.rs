// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Driver-agnostic helpers shared by every SQL storage backend.
//!
//! The `SQLite`, `Postgres` and `MySQL` backends all translate the same etcd
//! semantics onto SQL; only the wire/exec API differs. The *pure* pieces of that
//! translation — key validation, the `LIKE` pattern for a range, interval
//! containment, request validation, and read-revision resolution — are identical
//! across all three and live here so there is exactly one verified copy. The
//! `SQLite` backend's tests exercise these against a real database.

#![cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]

use crate::error::{KineError, Result};
use crate::range::{RangeEnd, RangeRequest};
use crate::revision::Revision;

/// Map a key's bytes to the UTF-8 string the `name` column stores (kine keys are
/// always valid UTF-8 registry paths). Rejects an empty or non-UTF-8 key.
pub(crate) fn key_str(key: &[u8]) -> Result<String> {
    if key.is_empty() {
        return Err(KineError::EmptyKey);
    }
    String::from_utf8(key.to_vec())
        .map_err(|_| KineError::Backend { message: "key is not valid UTF-8".into() })
}

/// The `LIKE` pattern for a request's interval: exact for a point get, `p%` for a
/// prefix, `%` for the whole keyspace, and the broadest safe prefix for an
/// explicit interval (then post-filtered in [`contains`]).
pub(crate) fn like_pattern(req: &RangeRequest) -> String {
    match &req.end {
        RangeEnd::Single => String::from_utf8_lossy(&req.key).into_owned(),
        RangeEnd::Prefix => format!("{}%", String::from_utf8_lossy(&req.key)),
        RangeEnd::AllKeys => "%".to_string(),
        RangeEnd::Explicit(end) => {
            let common = common_prefix(&req.key, end);
            format!("{}%", String::from_utf8_lossy(common))
        }
    }
}

/// The shared leading bytes of two keys.
fn common_prefix<'a>(a: &'a [u8], b: &[u8]) -> &'a [u8] {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    &a[..n]
}

/// Does `candidate` fall in `req`'s interval? Mirrors `RangeRequest::contains`.
pub(crate) fn contains(req: &RangeRequest, candidate: &[u8]) -> bool {
    match &req.end {
        RangeEnd::Single => candidate == req.key.as_slice(),
        RangeEnd::Prefix => candidate.starts_with(&req.key),
        RangeEnd::AllKeys => true,
        RangeEnd::Explicit(end) => candidate >= req.key.as_slice() && candidate < end.as_slice(),
    }
}

/// Validate a range request the same way [`crate::range::execute`] does.
pub(crate) fn validate_range(req: &RangeRequest) -> Result<()> {
    if req.limit < 0 {
        return Err(KineError::NegativeLimit { limit: req.limit });
    }
    match &req.end {
        RangeEnd::AllKeys => Ok(()),
        RangeEnd::Single | RangeEnd::Prefix => {
            if req.key.is_empty() {
                Err(KineError::EmptyKey)
            } else {
                Ok(())
            }
        }
        RangeEnd::Explicit(end) => {
            if req.key.is_empty() {
                Err(KineError::EmptyKey)
            } else if end.as_slice() <= req.key.as_slice() {
                Err(KineError::InvalidRange)
            } else {
                Ok(())
            }
        }
    }
}

/// Resolve a request revision against the header (mirrors `Clock::resolve_read`).
pub(crate) const fn resolve_read(requested: Revision, header: Revision) -> Result<Revision> {
    if requested < 0 {
        return Err(KineError::NegativeRevision { revision: requested });
    }
    if requested == 0 {
        return Ok(header);
    }
    if requested > header {
        return Err(KineError::FutureRevision { requested, current: header });
    }
    Ok(requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_str_rejects_empty_and_non_utf8() {
        assert_eq!(key_str(b""), Err(KineError::EmptyKey));
        assert!(matches!(key_str(&[0xff, 0xfe]), Err(KineError::Backend { .. })));
        assert_eq!(key_str(b"/reg/a").unwrap(), "/reg/a");
    }

    #[test]
    fn like_pattern_covers_every_range_shape() {
        assert_eq!(like_pattern(&RangeRequest::key(b"/a")), "/a");
        assert_eq!(like_pattern(&RangeRequest::prefix(b"/ns/")), "/ns/%");
        assert_eq!(like_pattern(&RangeRequest::all()), "%");
        assert_eq!(like_pattern(&RangeRequest::interval(b"/a/x", b"/a/z")), "/a/%");
    }

    #[test]
    fn contains_honours_interval_bounds() {
        assert!(contains(&RangeRequest::prefix(b"/ns/"), b"/ns/x"));
        assert!(!contains(&RangeRequest::prefix(b"/ns/"), b"/other"));
        assert!(contains(&RangeRequest::interval(b"/a", b"/c"), b"/b"));
        assert!(!contains(&RangeRequest::interval(b"/a", b"/c"), b"/c"));
    }

    #[test]
    fn resolve_read_guards_future_and_negative_revisions() {
        assert_eq!(resolve_read(0, 5), Ok(5));
        assert_eq!(resolve_read(3, 5), Ok(3));
        assert_eq!(resolve_read(9, 5), Err(KineError::FutureRevision { requested: 9, current: 5 }));
        assert_eq!(resolve_read(-1, 5), Err(KineError::NegativeRevision { revision: -1 }));
    }

    #[test]
    fn validate_range_rejects_empty_key_and_inverted_interval() {
        assert_eq!(validate_range(&RangeRequest::key(b"")), Err(KineError::EmptyKey));
        assert_eq!(validate_range(&RangeRequest::interval(b"/z", b"/a")), Err(KineError::InvalidRange));
        assert!(validate_range(&RangeRequest::all()).is_ok());
    }
}
