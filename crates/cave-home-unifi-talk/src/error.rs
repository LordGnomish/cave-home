// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// HA core has no `unifi_talk` integration in 2026.5.2; the error shape
// follows the cave-home unifi-network / unifi-protect convention.

use thiserror::Error;

/// Crate-wide `Result` alias.
pub type TalkResult<T> = Result<T, TalkError>;

/// UniFi Talk error surface.
#[derive(Debug, Error)]
pub enum TalkError {
    /// Authentication failed.
    #[error("authentication required: {0}")]
    Auth(String),

    /// Cannot reach the Talk REST endpoint.
    #[error("cannot connect to UniFi Talk at {0}")]
    Connect(String),

    /// Timeout.
    #[error("timeout talking to UniFi Talk")]
    Timeout,

    /// Unknown call / phone / extension.
    #[error("not found: {0}")]
    NotFound(String),

    /// Call control verb rejected (call already ended, transferred, etc.).
    #[error("call control failed: {0}")]
    CallControl(String),

    /// API surface unavailable — Ubiquiti has not exposed this capability
    /// in the public API yet. Treated as a hard "won't fix in Phase 1".
    #[error("API surface unavailable: {0}")]
    Unavailable(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_is_descriptive() {
        let e = TalkError::Unavailable("voicemail".into());
        assert!(e.to_string().contains("API surface unavailable"));
    }
}
