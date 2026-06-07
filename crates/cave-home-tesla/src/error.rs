// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The crate-wide error type and `Result` alias.
//!
//! [`TeslaError`] is deliberately small and never embeds a token, password or
//! authorization header in its `Display` — the auth layer constructs
//! [`TeslaError::Auth`] from a *reason*, not from the secret material.

use thiserror::Error;

/// The crate-wide result type.
pub type Result<T> = core::result::Result<T, TeslaError>;

/// Everything that can go wrong talking to a Tesla energy site.
#[derive(Debug, Error)]
pub enum TeslaError {
    /// The `OAuth2` / token exchange failed. Carries the upstream *reason* code
    /// (e.g. `invalid_grant`), never the secret material.
    #[error("authentication failed: {0}")]
    Auth(String),

    /// The caller (or the rate limiter) must wait before retrying.
    #[error("rate limited; retry in {retry_after_secs}s")]
    RateLimited {
        /// How long to wait before the next request to this endpoint.
        retry_after_secs: u64,
    },

    /// The access token is missing, expired or rejected (HTTP 401/403).
    #[error("not authorized (token missing, expired or rejected)")]
    Unauthorized,

    /// A non-success HTTP status that is not one of the specially-handled cases.
    #[error("unexpected HTTP {status}: {body}")]
    Http {
        /// The HTTP status code.
        status: u16,
        /// A short, non-secret excerpt of the response body.
        body: String,
    },

    /// A response body could not be decoded into the expected shape.
    #[error("could not decode response: {0}")]
    Decode(String),

    /// The underlying transport (socket / TLS) failed.
    #[error("transport error: {0}")]
    Transport(String),

    /// A required piece of configuration is absent.
    #[error("not configured: {0}")]
    NotConfigured(String),

    /// A caller-supplied value is out of range or otherwise invalid.
    #[error("invalid value: {0}")]
    Validation(String),

    /// No cached state was available to serve while the API was unreachable.
    #[error("no cached state available")]
    CacheMiss,
}

impl From<serde_json::Error> for TeslaError {
    fn from(e: serde_json::Error) -> Self {
        Self::Decode(e.to_string())
    }
}

impl TeslaError {
    /// Map an HTTP status (and a short body excerpt) onto the most specific
    /// variant: 401/403 → [`Unauthorized`](Self::Unauthorized), 429 →
    /// [`RateLimited`](Self::RateLimited) (default 30 s), everything else →
    /// [`Http`](Self::Http).
    #[must_use]
    pub fn from_status(status: u16, body: &str) -> Self {
        match status {
            401 | 403 => Self::Unauthorized,
            429 => Self::RateLimited { retry_after_secs: 30 },
            _ => Self::Http {
                status,
                body: body.to_string(),
            },
        }
    }

    /// Whether retrying the same request later could plausibly succeed: rate
    /// limits, transport blips and 5xx server errors are transient; validation,
    /// missing config, auth rejection and 4xx are terminal.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::Transport(_) => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_human_readable() {
        let e = TeslaError::RateLimited { retry_after_secs: 30 };
        assert!(e.to_string().contains("30"));
        let e = TeslaError::Http {
            status: 503,
            body: "upstream down".into(),
        };
        assert!(e.to_string().contains("503"));
    }

    #[test]
    fn retryable_classifies_transient_failures() {
        assert!(TeslaError::RateLimited { retry_after_secs: 1 }.is_retryable());
        assert!(TeslaError::Transport("reset".into()).is_retryable());
        assert!(TeslaError::Http { status: 500, body: String::new() }.is_retryable());
        assert!(TeslaError::Http { status: 502, body: String::new() }.is_retryable());
    }

    #[test]
    fn retryable_excludes_client_and_logic_errors() {
        assert!(!TeslaError::Validation("percent > 100".into()).is_retryable());
        assert!(!TeslaError::NotConfigured("site_id".into()).is_retryable());
        assert!(!TeslaError::Unauthorized.is_retryable());
        assert!(!TeslaError::Http { status: 400, body: String::new() }.is_retryable());
        assert!(!TeslaError::Decode("bad json".into()).is_retryable());
    }

    #[test]
    fn json_error_maps_to_decode() {
        let err: TeslaError = serde_json::from_str::<serde_json::Value>("{not json")
            .unwrap_err()
            .into();
        assert!(matches!(err, TeslaError::Decode(_)));
    }

    #[test]
    fn display_never_echoes_a_secret() {
        // The auth variant carries a reason, not the token.
        let e = TeslaError::Auth("invalid_grant".into());
        let shown = e.to_string();
        assert!(shown.contains("invalid_grant"));
        assert!(!shown.contains("Bearer"));
    }

    #[test]
    fn unauthorized_maps_from_status() {
        assert!(matches!(TeslaError::from_status(401, ""), TeslaError::Unauthorized));
        assert!(matches!(TeslaError::from_status(403, ""), TeslaError::Unauthorized));
        assert!(matches!(
            TeslaError::from_status(429, ""),
            TeslaError::RateLimited { .. }
        ));
        assert!(matches!(
            TeslaError::from_status(500, "boom"),
            TeslaError::Http { status: 500, .. }
        ));
    }
}
