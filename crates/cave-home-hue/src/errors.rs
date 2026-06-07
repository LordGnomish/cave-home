// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/errors.py
// Source: home-assistant/core@2026.5.2 homeassistant/components/hue/errors.py
//! Hue error taxonomy. Mirrors `aiohue.errors` line-by-line.
//!
//! Reference: <https://developers.meethue.com/documentation/error-messages>

use thiserror::Error;

/// Base exception type for the Hue stack. Matches `aiohue.AiohueException`.
#[derive(Debug, Error)]
pub enum HueError {
    /// Application key not authorised by the bridge. Maps to error type 1.
    /// Upstream: `aiohue.errors.Unauthorized`.
    #[error("unauthorised: {0}")]
    Unauthorized(String),

    /// The user tried to pair while the link button had not been pressed.
    /// Maps to error type 101. Upstream: `aiohue.errors.LinkButtonNotPressed`.
    #[error("link button not pressed: {0}")]
    LinkButtonNotPressed(String),

    /// We received an EventStream event we cannot (yet) handle.
    /// Upstream: `aiohue.errors.InvalidEvent`.
    #[error("invalid event: {0}")]
    InvalidEvent(String),

    /// Trying to connect to an unsupported bridge API version.
    /// Upstream: `aiohue.errors.InvalidAPIVersion`.
    #[error("invalid API version: {0}")]
    InvalidApiVersion(String),

    /// Repeated requests to the bridge failed (HTTP 503 / 429).
    /// Upstream: `aiohue.errors.BridgeBusy`.
    #[error("bridge busy: {0}")]
    BridgeBusy(String),

    /// Bridge firmware too old. Upstream: `aiohue.errors.BridgeSoftwareOutdated`.
    #[error("bridge software outdated: {0}")]
    BridgeSoftwareOutdated(String),

    /// Anything else from the bridge JSON `error` block.
    /// Upstream: `aiohue.errors.AiohueException`.
    #[error("Hue API error: {0}")]
    Generic(String),

    /// Transport / HTTP error (catch-all). HA's `bridge.py` maps a number of
    /// aiohttp exceptions to `ConfigEntryNotReady`; we surface them as
    /// `Transport` so the caller can decide retry vs. reauth.
    #[error("transport: {0}")]
    Transport(String),
}

/// Convenience alias used across the crate.
pub type HueResult<T> = Result<T, HueError>;

/// Map a Hue JSON error object to a typed [`HueError`].
///
/// Upstream pattern (`aiohue.errors.raise_from_error`):
/// ```python
/// ERRORS = {1: Unauthorized, 101: LinkButtonNotPressed}
/// def raise_from_error(error: dict) -> AiohueException:
///     _type = error.get("type")
///     cls = ERRORS.get(_type, AiohueException)
///     raise cls(error["description"])
/// ```
#[must_use]
pub fn from_hue_error(type_code: Option<i64>, description: &str) -> HueError {
    match type_code {
        Some(1) => HueError::Unauthorized(description.to_string()),
        Some(101) => HueError::LinkButtonNotPressed(description.to_string()),
        _ => HueError::Generic(description.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_type_1_to_unauthorized() {
        let err = from_hue_error(Some(1), "unauthorized user");
        assert!(matches!(err, HueError::Unauthorized(_)));
    }

    #[test]
    fn maps_type_101_to_link_button() {
        let err = from_hue_error(Some(101), "link button not pressed");
        assert!(matches!(err, HueError::LinkButtonNotPressed(_)));
    }

    #[test]
    fn unknown_type_maps_to_generic() {
        let err = from_hue_error(Some(999), "wat");
        assert!(matches!(err, HueError::Generic(_)));
        let err = from_hue_error(None, "wat");
        assert!(matches!(err, HueError::Generic(_)));
    }

    #[test]
    fn error_messages_propagate_description() {
        let err = from_hue_error(Some(1), "you are not authorised");
        let s = format!("{err}");
        assert!(s.contains("you are not authorised"));
    }
}
