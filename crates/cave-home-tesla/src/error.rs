// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The crate-wide error type and `Result` alias.
//!
//! [`TeslaError`] is deliberately small and never embeds a token, password or
//! authorization header in its `Display` — the auth layer constructs
//! [`TeslaError::Auth`] from a *reason*, not from the secret material.

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
