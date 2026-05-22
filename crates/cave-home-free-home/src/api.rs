// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/api.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! SysAP REST client.
//!
//! The runtime HTTP wiring (TLS, basic-auth, backoff) lives in
//! `cave-home-binary` — this module exposes the *protocol* surface
//! (URL builders + request/response shapes) so it can be unit-tested
//! without spinning up an HTTP server. The transport trait
//! [`SysApTransport`] is what the binary plugs in.

use async_trait::async_trait;

use crate::channels::SetDatapointCommand;
use crate::error::Result;

/// API version exposed by the SysAP REST surface.
pub const API_VERSION: &str = "v1";

/// HTTP method used by the SysAP REST surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Put,
    Post,
}

/// Description of a single SysAP REST call. The transport implementation
/// is responsible for building the actual HTTP request from this DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysApRequest {
    pub method: Method,
    /// Path relative to the SysAP base URL (e.g. `/fhapi/v1/api/rest/configuration`).
    pub path: String,
    /// Optional JSON body (already serialized).
    pub body: Option<String>,
}

impl SysApRequest {
    /// Build the REST `GET /api/rest/configuration` request.
    #[must_use]
    pub fn get_configuration() -> Self {
        Self {
            method: Method::Get,
            path: format!("/fhapi/{API_VERSION}/api/rest/configuration"),
            body: None,
        }
    }

    /// Build the REST `GET /api/rest/sysap` (system info) request.
    #[must_use]
    pub fn get_sysap_settings() -> Self {
        Self {
            method: Method::Get,
            path: format!("/fhapi/{API_VERSION}/api/rest/sysap"),
            body: None,
        }
    }

    /// Build the REST `PUT /api/rest/datapoint/<sysap>/<serial>.<channel>.<dp>`
    /// request that ships a value change.
    #[must_use]
    pub fn put_datapoint(sysap_uuid: &str, cmd: &SetDatapointCommand) -> Self {
        Self {
            method: Method::Put,
            path: format!(
                "/fhapi/{API_VERSION}/api/rest/datapoint/{sysap_uuid}/{}.{}.{}",
                cmd.device_serial, cmd.channel_id, cmd.datapoint
            ),
            body: Some(cmd.value.clone()),
        }
    }

    /// Build the REST `GET /api/rest/devicelist` request.
    #[must_use]
    pub fn get_device_list() -> Self {
        Self {
            method: Method::Get,
            path: format!("/fhapi/{API_VERSION}/api/rest/devicelist"),
            body: None,
        }
    }
}

/// Transport plugged in by the cave-home binary; lets the rest of the
/// crate stay sans-IO and unit-testable.
#[async_trait]
pub trait SysApTransport: Send + Sync {
    async fn execute(&self, request: SysApRequest) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_path_is_stable() {
        let r = SysApRequest::get_configuration();
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.path, "/fhapi/v1/api/rest/configuration");
    }

    #[test]
    fn put_datapoint_builds_canonical_path() {
        let cmd = SetDatapointCommand {
            device_serial: "ABB7F500BCFB".into(),
            channel_id: "ch0000".into(),
            datapoint: "idp0000".into(),
            value: "1".into(),
        };
        let r = SysApRequest::put_datapoint("00000000-0000-0000-0000-000000000000", &cmd);
        assert_eq!(r.method, Method::Put);
        assert_eq!(
            r.path,
            "/fhapi/v1/api/rest/datapoint/\
             00000000-0000-0000-0000-000000000000/\
             ABB7F500BCFB.ch0000.idp0000"
        );
        assert_eq!(r.body.as_deref(), Some("1"));
    }
}
