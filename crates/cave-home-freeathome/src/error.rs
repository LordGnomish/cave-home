// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Error type for the free@home SysAP client.

use thiserror::Error;

/// Anything that can go wrong talking to a System Access Point.
#[derive(Debug, Error)]
pub enum FreeAtHomeError {
    /// An HTTP/REST transport failure (connection, status, body).
    #[error("http transport error: {0}")]
    Http(String),
    /// A WebSocket transport or handshake failure.
    #[error("websocket error: {0}")]
    WebSocket(String),
    /// Authentication was rejected by the SysAP.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// A response body could not be decoded into the expected shape.
    #[error("decode error: {0}")]
    Decode(String),
    /// A free@home domain rule was violated (wraps a `cave-home-free-home` error).
    #[error("free@home domain error: {0}")]
    Domain(String),
    /// The supplied configuration is invalid.
    #[error("configuration error: {0}")]
    Config(String),
}

impl From<serde_json::Error> for FreeAtHomeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Decode(e.to_string())
    }
}

/// The crate-wide result alias.
pub type Result<T> = core::result::Result<T, FreeAtHomeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_error_displays() {
        let e = FreeAtHomeError::Http("boom".into());
        assert_eq!(e.to_string(), "http transport error: boom");
    }

    #[test]
    fn auth_error_displays() {
        let e = FreeAtHomeError::Auth("bad credentials".into());
        assert_eq!(e.to_string(), "authentication failed: bad credentials");
    }

    #[test]
    fn is_a_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(FreeAtHomeError::Config("x".into()));
        assert!(e.to_string().contains("configuration"));
    }
}
