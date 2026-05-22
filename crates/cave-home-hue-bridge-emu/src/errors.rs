// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! Emulator-side error type + Hue protocol error helpers.
//!
//! Reference: developers.meethue.com/documentation/error-messages — every
//! v1 error code below maps to a published numeric type. v2 errors are
//! described in the CLIP API reference response envelope (`errors[].description`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level error from the emulator. Internal — the wire protocol always
/// surfaces a typed Hue error object (see [`HueProtocolError`]).
#[derive(Debug, Error)]
pub enum EmuError {
    /// The application key is not registered with the bridge.
    #[error("unauthorized application key")]
    Unauthorized,
    /// The pairing link button hasn't been pressed.
    #[error("link button not pressed")]
    LinkButtonNotPressed,
    /// The resource (light / group / scene / sensor / device) does not exist.
    #[error("resource not found: {0}")]
    NotFound(String),
    /// The request body failed schema validation.
    #[error("invalid body: {0}")]
    InvalidBody(String),
    /// Internal storage error.
    #[error("storage: {0}")]
    Storage(String),
    /// Anything else.
    #[error("{0}")]
    Other(String),
}

/// Hue v1 wire-format error object. Mirrors the published error schema.
///
/// Reference: `developers.meethue.com/documentation/error-messages`.
/// The bridge wraps any error as:
/// `[{"error": {"type": <int>, "address": "<path>", "description": "..."}}]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HueProtocolError {
    /// Hue error code. See [`HueErrorType`].
    #[serde(rename = "type")]
    pub kind: i64,
    /// Path that produced the error. May be empty for top-level failures.
    #[serde(default)]
    pub address: String,
    /// Human-readable description.
    pub description: String,
}

/// Subset of the v1 Hue error codes the emulator emits.
///
/// Reference: Philips developer-portal "Error messages" table.
/// We include the codes we generate; the published table is broader.
#[allow(missing_docs)]
pub mod error_type {
    pub const UNAUTHORIZED_USER: i64 = 1;
    pub const BODY_CONTAINS_INVALID_JSON: i64 = 2;
    pub const RESOURCE_NOT_AVAILABLE: i64 = 3;
    pub const METHOD_NOT_AVAILABLE: i64 = 4;
    pub const MISSING_PARAMETER_IN_BODY: i64 = 5;
    pub const PARAMETER_NOT_AVAILABLE: i64 = 6;
    pub const INVALID_VALUE_FOR_PARAMETER: i64 = 7;
    pub const PARAMETER_NOT_MODIFIABLE: i64 = 8;
    pub const TOO_MANY_ITEMS_IN_LIST: i64 = 11;
    pub const LINK_BUTTON_NOT_PRESSED: i64 = 101;
    pub const INTERNAL_ERROR: i64 = 901;
}

impl HueProtocolError {
    /// Build an `unauthorized user` error.
    #[must_use]
    pub fn unauthorized(address: impl Into<String>) -> Self {
        Self {
            kind: error_type::UNAUTHORIZED_USER,
            address: address.into(),
            description: "unauthorized user".into(),
        }
    }
    /// Build a `link button not pressed` error.
    #[must_use]
    pub fn link_button_not_pressed() -> Self {
        Self {
            kind: error_type::LINK_BUTTON_NOT_PRESSED,
            address: String::new(),
            description: "link button not pressed".into(),
        }
    }
    /// Build a `resource, /path, not available` error.
    #[must_use]
    pub fn not_found(address: impl Into<String>) -> Self {
        let address = address.into();
        Self {
            kind: error_type::RESOURCE_NOT_AVAILABLE,
            description: format!("resource, {address}, not available"),
            address,
        }
    }
    /// Build an `invalid value for parameter` error.
    #[must_use]
    pub fn invalid_value(address: impl Into<String>, value: impl Into<String>) -> Self {
        let address = address.into();
        let value = value.into();
        Self {
            kind: error_type::INVALID_VALUE_FOR_PARAMETER,
            description: format!("invalid value, {value}, for parameter, {address}"),
            address,
        }
    }
    /// Build a generic internal error.
    #[must_use]
    pub fn internal(description: impl Into<String>) -> Self {
        Self {
            kind: error_type::INTERNAL_ERROR,
            address: String::new(),
            description: description.into(),
        }
    }
}

/// Map an [`EmuError`] to a list of wire-format errors at a given path.
#[must_use]
pub fn emu_to_protocol(err: &EmuError, address: &str) -> HueProtocolError {
    match err {
        EmuError::Unauthorized => HueProtocolError::unauthorized(address),
        EmuError::LinkButtonNotPressed => HueProtocolError::link_button_not_pressed(),
        EmuError::NotFound(_) => HueProtocolError::not_found(address),
        EmuError::InvalidBody(desc) => HueProtocolError {
            kind: error_type::BODY_CONTAINS_INVALID_JSON,
            address: address.into(),
            description: desc.clone(),
        },
        EmuError::Storage(d) | EmuError::Other(d) => HueProtocolError::internal(d.clone()),
    }
}

/// Common alias.
pub type EmuResult<T> = Result<T, EmuError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_protocol_error_serialises_with_type_1() {
        let err = HueProtocolError::unauthorized("/api/abc/lights");
        let body = serde_json::to_value(&err).unwrap();
        assert_eq!(body.get("type").unwrap(), &serde_json::Value::from(1));
        assert_eq!(body.get("description").unwrap(), "unauthorized user");
    }

    #[test]
    fn not_found_protocol_error_describes_path() {
        let err = HueProtocolError::not_found("/lights/99");
        assert_eq!(err.kind, error_type::RESOURCE_NOT_AVAILABLE);
        assert!(err.description.contains("/lights/99"));
    }

    #[test]
    fn link_button_protocol_error_uses_code_101() {
        let err = HueProtocolError::link_button_not_pressed();
        assert_eq!(err.kind, 101);
    }

    #[test]
    fn emu_to_protocol_maps_categories() {
        let p = emu_to_protocol(&EmuError::Unauthorized, "/api/x");
        assert_eq!(p.kind, 1);
        let p = emu_to_protocol(&EmuError::LinkButtonNotPressed, "");
        assert_eq!(p.kind, 101);
        let p = emu_to_protocol(&EmuError::NotFound("light".into()), "/lights/9");
        assert_eq!(p.kind, 3);
        let p = emu_to_protocol(&EmuError::InvalidBody("bad json".into()), "/api");
        assert_eq!(p.kind, 2);
        let p = emu_to_protocol(&EmuError::Other("boom".into()), "");
        assert_eq!(p.kind, 901);
    }
}
