// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/exceptions.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! Errors raised by the free@home client.

use thiserror::Error;

/// All errors the crate can raise.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum FreeAtHomeError {
    /// Could not reach the SysAP (`ClientConnectionError`).
    #[error("cannot connect to host {0}")]
    ClientConnection(String),

    /// SSL certificate verification failed (`SslErrorException`).
    #[error("SSL certificate verification failed for host {0}")]
    Ssl(String),

    /// Connection timed out (`ConnectionTimeoutException`).
    #[error("connection timeout to host: {0}")]
    Timeout(String),

    /// SysAP returned 403 (`ForbiddenAuthException`).
    #[error("request returned forbidden (status {status}): {path}")]
    Forbidden { path: String, status: u16 },

    /// Bad username/password (`InvalidCredentialsException`).
    #[error("invalid credentials for user: {0}")]
    InvalidCredentials(String),

    /// Malformed host URL (`InvalidHostException`).
    #[error("invalid host endpoint, ensure URL includes schema (e.g. http://): {0}")]
    InvalidHost(String),

    /// Server returned a non-success status (`InvalidApiResponseException`).
    #[error("invalid API response, status code: {0}")]
    InvalidApiResponse(u16),

    /// Pairing ID not found on the channel (`InvalidDeviceChannelPairing`).
    #[error("could not find pairing id for device {device_serial} channel {channel_id} pairing {pairing_value}")]
    InvalidPairing {
        device_serial: String,
        channel_id: String,
        pairing_value: u32,
    },

    /// `set_datapoint` rejected by SysAP (`SetDatapointFailureException`).
    #[error("set_datapoint failed for {device_serial}/{channel_id}/{datapoint}")]
    SetDatapointFailure {
        device_serial: String,
        channel_id: String,
        datapoint: String,
    },

    /// JSON encode/decode failure.
    #[error("JSON error: {0}")]
    Json(String),
}

/// Crate result alias.
pub type Result<T> = core::result::Result<T, FreeAtHomeError>;
