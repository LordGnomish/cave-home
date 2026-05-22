// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/errors.py +
//                            homeassistant/components/unifi/hub/api.py
//
// HA raises four distinct exception classes (AuthenticationRequired,
// CannotConnect, ConfigEntryAuthFailed, ConfigEntryNotReady). Rust port
// widens them into one typed enum.

use thiserror::Error;

/// Crate-wide `Result` alias.
pub type UnifiResult<T> = Result<T, UnifiError>;

/// UniFi Network controller error surface.
#[derive(Debug, Error)]
pub enum UnifiError {
    /// Authentication failed (HA: `AuthenticationRequired` ←
    /// `aiounifi.Unauthorized`, `aiounifi.LoginRequired`).
    #[error("authentication required: {0}")]
    Auth(String),

    /// Connection refused / unreachable (HA: `CannotConnect` ←
    /// `aiounifi.BadGateway`, `aiounifi.RequestError`, `TimeoutError`).
    #[error("cannot connect to UniFi controller at {0}")]
    Connect(String),

    /// Login timeout (HA: `asyncio.timeout(10)` wrapper around
    /// `api.login()`).
    #[error("timeout talking to UniFi controller")]
    Timeout,

    /// Controller returned a non-2xx response we cannot parse.
    #[error("response error: {0}")]
    Response(String),

    /// JSON decode error.
    #[error("json decode failed: {0}")]
    Decode(String),

    /// The controller is healthy but the requested resource is unknown.
    #[error("unknown resource: {0}")]
    NotFound(String),

    /// WebSocket subscription lost.
    #[error("websocket lost: {0}")]
    WebSocketLost(String),
}

impl UnifiError {
    /// Convenience: wrap any string-y connect failure.
    pub fn connect<S: Into<String>>(s: S) -> Self {
        Self::Connect(s.into())
    }

    /// Convenience: wrap any string-y auth failure.
    pub fn auth<S: Into<String>>(s: S) -> Self {
        Self::Auth(s.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_are_distinct() {
        let a = UnifiError::auth("bad creds");
        let c = UnifiError::connect("refused");
        let t = UnifiError::Timeout;
        assert!(a.to_string().contains("authentication required"));
        assert!(c.to_string().contains("cannot connect"));
        assert_eq!(t.to_string(), "timeout talking to UniFi controller");
    }
}
