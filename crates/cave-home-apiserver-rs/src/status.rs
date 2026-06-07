// SPDX-License-Identifier: Apache-2.0
//! API status / error model — the `metav1.Status` shape.
//!
//! Behavioural reference: Kubernetes API conventions document
//! (`api-conventions.md`, "Response Status Kind") and the documented
//! `StatusReason` → HTTP code mapping. This is a clean-room reimplementation of
//! the *documented* contract, not a transcription of upstream Go source.

use std::fmt;

/// Machine-readable reason for a failed request. Mirrors the documented
/// `metav1.StatusReason` set the apiserver returns. Each reason carries a
/// canonical HTTP status code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusReason {
    /// 404 — the resource (or its collection) does not exist.
    NotFound,
    /// 409 — a create collided with an existing object of the same name.
    AlreadyExists,
    /// 409 — an update lost the optimistic-concurrency race (stale
    /// resourceVersion) or otherwise conflicts with server state.
    Conflict,
    /// 422 — the object failed validation / admission.
    Invalid,
    /// 400 — the request itself was malformed.
    BadRequest,
    /// 401 — the request presented no credentials, or invalid ones.
    Unauthorized,
    /// 403 — authenticated but not authorized.
    Forbidden,
    /// 405 — the verb is not supported on this resource.
    MethodNotAllowed,
    /// 500 — an unexpected internal error.
    InternalError,
}

impl StatusReason {
    /// The canonical HTTP status code for this reason.
    #[must_use]
    pub fn code(self) -> u16 {
        match self {
            StatusReason::BadRequest => 400,
            StatusReason::Unauthorized => 401,
            StatusReason::Forbidden => 403,
            StatusReason::NotFound => 404,
            StatusReason::MethodNotAllowed => 405,
            StatusReason::AlreadyExists | StatusReason::Conflict => 409,
            StatusReason::Invalid => 422,
            StatusReason::InternalError => 500,
        }
    }

    /// The wire token (the `reason` field value).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            StatusReason::NotFound => "NotFound",
            StatusReason::AlreadyExists => "AlreadyExists",
            StatusReason::Conflict => "Conflict",
            StatusReason::Invalid => "Invalid",
            StatusReason::BadRequest => "BadRequest",
            StatusReason::Unauthorized => "Unauthorized",
            StatusReason::Forbidden => "Forbidden",
            StatusReason::MethodNotAllowed => "MethodNotAllowed",
            StatusReason::InternalError => "InternalError",
        }
    }
}

/// A failed-request status (`status: "Failure"`). Returned as the `Err` variant
/// from every REST operation; carries the reason, derived HTTP code and a
/// human-readable message. No panics: this is the universal error type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Status {
    /// Machine reason.
    pub reason: StatusReason,
    /// HTTP status code (derived from `reason`).
    pub code: u16,
    /// Human-readable detail.
    pub message: String,
}

impl Status {
    /// Construct a failure status from a reason and message.
    #[must_use]
    pub fn new(reason: StatusReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            code: reason.code(),
            message: message.into(),
        }
    }

    /// 404 helper.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusReason::NotFound, message)
    }

    /// 409 AlreadyExists helper.
    #[must_use]
    pub fn already_exists(message: impl Into<String>) -> Self {
        Self::new(StatusReason::AlreadyExists, message)
    }

    /// 409 Conflict helper.
    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusReason::Conflict, message)
    }

    /// 422 Invalid helper.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusReason::Invalid, message)
    }

    /// 400 BadRequest helper.
    #[must_use]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusReason::BadRequest, message)
    }

    /// 401 Unauthorized helper.
    #[must_use]
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusReason::Unauthorized, message)
    }

    /// 403 Forbidden helper.
    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusReason::Forbidden, message)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.reason.as_str(), self.code, self.message)
    }
}

impl std::error::Error for Status {}

/// Result alias used across the decision core.
pub type Result<T> = std::result::Result<T, Status>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_codes_match_k8s_conventions() {
        assert_eq!(StatusReason::NotFound.code(), 404);
        assert_eq!(StatusReason::AlreadyExists.code(), 409);
        assert_eq!(StatusReason::Conflict.code(), 409);
        assert_eq!(StatusReason::Invalid.code(), 422);
        assert_eq!(StatusReason::BadRequest.code(), 400);
        assert_eq!(StatusReason::MethodNotAllowed.code(), 405);
        assert_eq!(StatusReason::InternalError.code(), 500);
    }

    #[test]
    fn helpers_set_reason_and_code() {
        let s = Status::not_found("pods \"x\" not found");
        assert_eq!(s.reason, StatusReason::NotFound);
        assert_eq!(s.code, 404);
        assert!(s.message.contains("not found"));
    }

    #[test]
    fn display_includes_reason_and_code() {
        let s = Status::conflict("stale");
        assert_eq!(s.to_string(), "Conflict (409): stale");
    }
}
