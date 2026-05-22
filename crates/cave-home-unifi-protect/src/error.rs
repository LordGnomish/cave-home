// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifiprotect/data.py
//                            + uiprotect.exceptions.{ClientError, NotAuthorized}

use thiserror::Error;

/// Crate-wide `Result` alias.
pub type ProtectResult<T> = Result<T, ProtectError>;

/// UniFi Protect NVR error surface.
#[derive(Debug, Error)]
pub enum ProtectError {
    /// Authentication failed (HA: `NotAuthorized`).
    #[error("authentication required: {0}")]
    Auth(String),

    /// Network unreachable (HA: `ClientError`).
    #[error("cannot connect to UniFi Protect NVR at {0}")]
    Connect(String),

    /// Login timeout.
    #[error("timeout talking to UniFi Protect")]
    Timeout,

    /// Protocol mismatch — NVR firmware predates v6 (HA:
    /// `OUTDATED_LOG_MESSAGE`).
    #[error("UniFi Protect version too old: {got}, need ≥ {needed}")]
    VersionTooOld {
        /// Version string from the NVR.
        got: String,
        /// Required minimum.
        needed: String,
    },

    /// JSON decode error.
    #[error("decode failed: {0}")]
    Decode(String),

    /// Unknown camera / NVR resource.
    #[error("unknown resource: {0}")]
    NotFound(String),

    /// WebSocket subscription lost.
    #[error("websocket lost: {0}")]
    WebSocketLost(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_too_old_format() {
        let e = ProtectError::VersionTooOld {
            got: "5.0.0".into(),
            needed: "6.0.0".into(),
        };
        assert!(e.to_string().contains("5.0.0"));
        assert!(e.to_string().contains("6.0.0"));
    }
}
