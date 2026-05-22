// SPDX-License-Identifier: Apache-2.0
//! Error types — port of `homeassistant/exceptions.py`.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/exceptions.py

use thiserror::Error;

/// Top-level error type for the cave-home automation engine.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/exceptions.py::HomeAssistantError
#[derive(Debug, Error)]
pub enum HassError {
    /// Entity ID does not match `<domain>.<object_id>` shape.
    ///
    /// # Upstream: homeassistant/exceptions.py::InvalidEntityFormatError
    #[error("invalid entity id: {0}; format should be <domain>.<object_id>")]
    InvalidEntityFormat(String),

    /// State string failed validation (length, control chars).
    ///
    /// # Upstream: homeassistant/exceptions.py::InvalidStateError
    #[error("invalid state value: {0}")]
    InvalidState(String),

    /// Service not registered.
    ///
    /// # Upstream: homeassistant/exceptions.py::ServiceNotFound
    #[error("service {domain}.{service} not found")]
    ServiceNotFound { domain: String, service: String },

    /// Service call data did not satisfy the registered schema.
    ///
    /// # Upstream: homeassistant/exceptions.py::ServiceValidationError
    #[error("service validation error: {0}")]
    ServiceValidationError(String),

    /// Template rendering failed.
    ///
    /// # Upstream: homeassistant/exceptions.py::TemplateError
    #[error("template error: {0}")]
    TemplateError(String),

    /// Condition evaluation failed.
    ///
    /// # Upstream: homeassistant/exceptions.py::ConditionError
    #[error("condition error: {0}")]
    ConditionError(String),

    /// Configuration is invalid — used by automation engine and config-entry
    /// flow handlers.
    #[error("config invalid: {0}")]
    ConfigInvalid(String),

    /// Unknown config entry handle.
    #[error("unknown config entry: {0}")]
    UnknownConfigEntry(String),

    /// JSON (de)serialisation failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// Generic platform / integration error wrapping an arbitrary message.
    ///
    /// # Upstream: homeassistant/exceptions.py::HomeAssistantError
    #[error("{0}")]
    Other(String),
}

/// Convenience [`Result`] alias.
pub type HassResult<T> = Result<T, HassError>;

impl HassError {
    /// Build an `Other(...)` from a `Display`-able value.
    pub fn msg(value: impl std::fmt::Display) -> Self {
        Self::Other(value.to_string())
    }
}
