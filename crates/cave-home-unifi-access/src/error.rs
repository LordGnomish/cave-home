// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi_access/__init__.py
//                            + unifi_access_api.{ApiAuthError, ApiConnectionError, ApiError, ApiNotFoundError}

use thiserror::Error;

/// Crate-wide `Result` alias.
pub type AccessResult<T> = Result<T, AccessError>;

/// UniFi Access error surface.
#[derive(Debug, Error)]
pub enum AccessError {
    /// Authentication failed (HA: `ApiAuthError`).
    #[error("authentication required: {0}")]
    Auth(String),

    /// Cannot reach the UniFi Access hub (HA: `ApiConnectionError`).
    #[error("cannot connect to UniFi Access at {0}")]
    Connect(String),

    /// Generic API error (HA: `ApiError`).
    #[error("API error: {0}")]
    Api(String),

    /// Resource not found (HA: `ApiNotFoundError`).
    #[error("not found: {0}")]
    NotFound(String),

    /// Request timed out.
    #[error("timeout talking to UniFi Access")]
    Timeout,

    /// Invalid lock-rule type passed in.
    #[error("invalid lock rule type: {0}")]
    InvalidLockRuleType(String),

    /// WebSocket subscription lost.
    #[error("websocket lost: {0}")]
    WebSocketLost(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_rule_type_message() {
        let e = AccessError::InvalidLockRuleType("foo".into());
        assert!(e.to_string().contains("invalid lock rule type"));
    }
}
