// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The crate-wide error type.
//!
//! Everything that can go wrong talking to a UniFi console funnels through
//! [`UnifiError`]: a transport failure (DNS/TLS/connect), an HTTP status the
//! API rejected us with (401/403/404/429/5xx), a body we could not decode, a
//! login/CSRF problem, or a WebSocket fault. The variants carry enough to act
//! on — a 401 means "re-login", a 429 means "back off" — without leaking the
//! raw response to the household.

use thiserror::Error;

/// The result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, UnifiError>;

/// Everything that can go wrong talking to a UniFi console.
#[derive(Debug, Error)]
pub enum UnifiError {
    /// The underlying transport (DNS, TCP, TLS, timeout) failed before we got
    /// any HTTP response.
    #[error("transport error: {0}")]
    Transport(String),

    /// The console answered with a non-success HTTP status. `body` is the raw
    /// (already length-capped) response text, kept for diagnostics.
    #[error("http {status}: {message}")]
    Http {
        /// The HTTP status code.
        status: u16,
        /// A short human message (the API's `meta.msg` when present, else the
        /// reason phrase).
        message: String,
        /// The raw body, capped.
        body: String,
    },

    /// We are not (or no longer) authenticated: no session, or the console
    /// returned 401. The caller should (re-)login.
    #[error("not authenticated: {0}")]
    Unauthorized(String),

    /// Authenticated, but the console refused the action (403) — typically a
    /// missing CSRF token or insufficient role.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// The console rate-limited us (429). `retry_after_secs` is the
    /// `Retry-After` hint when the console sent one.
    #[error("rate limited (retry after {retry_after_secs:?}s)")]
    RateLimited {
        /// Seconds to wait, from the `Retry-After` header, if any.
        retry_after_secs: Option<u64>,
    },

    /// A response body did not match the shape we expected.
    #[error("decode error: {0}")]
    Decode(String),

    /// Login failed (bad credentials, MFA required, or a missing token/CSRF in
    /// the login response).
    #[error("login failed: {0}")]
    Login(String),

    /// A WebSocket-layer fault (handshake, frame, or unexpected close).
    #[error("websocket error: {0}")]
    WebSocket(String),

    /// A caller passed an argument the API would reject (empty site, bad URL).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl UnifiError {
    /// Map a raw HTTP status + message + body into the most specific variant.
    /// This is the single place status→error policy lives, so every API call
    /// classifies failures the same way.
    #[must_use]
    pub fn from_status(status: u16, message: impl Into<String>, body: impl Into<String>) -> Self {
        let message = message.into();
        match status {
            401 => Self::Unauthorized(message),
            403 => Self::Forbidden(message),
            429 => Self::RateLimited {
                retry_after_secs: None,
            },
            _ => Self::Http {
                status,
                message,
                body: body.into(),
            },
        }
    }

    /// Whether re-authenticating and retrying could plausibly help. A 401 (lost
    /// session) is retryable; a 403 (genuinely forbidden) is not.
    #[must_use]
    pub fn is_auth_retryable(&self) -> bool {
        matches!(self, Self::Unauthorized(_))
    }

    /// Whether this is a transient condition a back-off-and-retry could clear.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) | Self::RateLimited { .. } => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_401_maps_to_unauthorized_and_is_retryable() {
        let e = UnifiError::from_status(401, "api.err.LoginRequired", "");
        assert!(matches!(e, UnifiError::Unauthorized(_)));
        assert!(e.is_auth_retryable());
        assert!(!e.is_transient());
    }

    #[test]
    fn status_403_maps_to_forbidden_not_retryable() {
        let e = UnifiError::from_status(403, "no csrf", "");
        assert!(matches!(e, UnifiError::Forbidden(_)));
        assert!(!e.is_auth_retryable());
    }

    #[test]
    fn status_429_maps_to_rate_limited_transient() {
        let e = UnifiError::from_status(429, "slow down", "");
        assert!(matches!(e, UnifiError::RateLimited { .. }));
        assert!(e.is_transient());
    }

    #[test]
    fn status_500_is_http_and_transient() {
        let e = UnifiError::from_status(500, "boom", "stacktrace");
        match &e {
            UnifiError::Http { status, body, .. } => {
                assert_eq!(*status, 500);
                assert_eq!(body, "stacktrace");
            }
            other => panic!("expected Http, got {other:?}"),
        }
        assert!(e.is_transient());
    }

    #[test]
    fn status_404_is_http_not_transient() {
        let e = UnifiError::from_status(404, "missing", "");
        assert!(matches!(e, UnifiError::Http { status: 404, .. }));
        assert!(!e.is_transient());
    }

    #[test]
    fn display_is_household_safe_and_compact() {
        let e = UnifiError::Login("MFA required".into());
        assert_eq!(e.to_string(), "login failed: MFA required");
    }
}
