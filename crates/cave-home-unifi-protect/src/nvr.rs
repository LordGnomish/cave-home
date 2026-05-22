// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifiprotect/__init__.py
//                            + homeassistant/components/unifiprotect/data.py
//                            + uiprotect.data.NVR
//
// HA's `data.py` `ProtectData` class is the runtime coordinator that
// owns the `ProtectApiClient`, the device dictionaries, and the WS
// subscription. cave-home Phase 1 ports the data shape (NvrConfig +
// ProtectNvr + ProtectClient); the WS coordinator is a Phase 2 ticket.

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::camera::ProtectCamera;
use crate::const_table::{DEFAULT_MAX_MEDIA, DEFAULT_PORT, DEFAULT_VERIFY_SSL};
use crate::error::{ProtectError, ProtectResult};
use crate::identifiers::{CameraId, NvrId};

/// UniFi Protect NVR connection config.
///
/// Source: HA `unifiprotect/config_flow.py` step_user form fields +
/// `unifiprotect/const.py` `CONF_*` keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvrConfig {
    /// NVR hostname or IP.
    pub host: String,
    /// Login username.
    pub username: String,
    /// Login password.
    pub password: String,
    /// HTTPS port (default 443; user must override for non-Cloud-Key).
    pub port: u16,
    /// Verify the NVR TLS cert (HA default: `False`).
    pub verify_ssl: bool,
    /// Set true to disable the RTSP stream surface entirely.
    pub disable_rtsp: bool,
    /// Override the connection host the NVR returns in its bootstrap
    /// payload (HA: `CONF_OVERRIDE_CHOST`).
    pub override_connection_host: Option<String>,
    /// Max media-source entries to surface (HA: `CONF_MAX_MEDIA`).
    pub max_media: u32,
    /// Allow Early-Access channels (HA: `CONF_ALLOW_EA`).
    pub allow_ea_channel: bool,
}

impl NvrConfig {
    /// Construct a config with the HA defaults applied.
    #[must_use]
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            username: username.into(),
            password: password.into(),
            port: DEFAULT_PORT,
            verify_ssl: DEFAULT_VERIFY_SSL,
            disable_rtsp: false,
            override_connection_host: None,
            max_media: DEFAULT_MAX_MEDIA,
            allow_ea_channel: false,
        }
    }

    /// Override the NVR port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Override the TLS verify flag.
    #[must_use]
    pub fn with_verify_ssl(mut self, verify: bool) -> Self {
        self.verify_ssl = verify;
        self
    }

    /// Override the disable-RTSP flag.
    #[must_use]
    pub fn with_disable_rtsp(mut self, disable: bool) -> Self {
        self.disable_rtsp = disable;
        self
    }

    /// Set the connection-host override.
    #[must_use]
    pub fn with_override_connection_host(mut self, host: impl Into<String>) -> Self {
        self.override_connection_host = Some(host.into());
        self
    }
}

/// A UniFi Protect NVR with its camera roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectNvr {
    /// NVR identifier.
    pub id: NvrId,
    /// User-set NVR label.
    pub label: String,
    /// Cameras currently adopted by this NVR.
    pub cameras: HashMap<CameraId, ProtectCamera>,
}

impl ProtectNvr {
    /// Construct an NVR with no cameras yet.
    #[must_use]
    pub fn new(id: NvrId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            cameras: HashMap::new(),
        }
    }

    /// Add or replace a camera in the NVR's roster.
    pub fn add_camera(&mut self, cam: ProtectCamera) {
        self.cameras.insert(cam.id.clone(), cam);
    }
}

/// UniFi Protect NVR client (ports HA `ProtectApiClient` wrapper).
///
/// Phase 1: validates the NVR is reachable via TCP probe with the same
/// 10-second timeout HA uses around `protect.update()`. Phase 2 ticket:
/// real REST bootstrap + WS subscription.
pub struct ProtectClient {
    cfg: NvrConfig,
    authenticated: Mutex<bool>,
    nvr: Mutex<Option<ProtectNvr>>,
}

impl ProtectClient {
    /// Construct an unauthenticated client.
    #[must_use]
    pub fn new(cfg: NvrConfig) -> Self {
        Self {
            cfg,
            authenticated: Mutex::new(false),
            nvr: Mutex::new(None),
        }
    }

    /// Borrow the NVR config.
    #[must_use]
    pub fn config(&self) -> &NvrConfig {
        &self.cfg
    }

    /// True if a successful `login()` has happened.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        *self.authenticated.lock()
    }

    /// Attempt to establish a session.
    pub async fn login(&mut self) -> ProtectResult<()> {
        let addr = format!("{}:{}", self.cfg.host, self.cfg.port);
        match timeout(Duration::from_secs(10), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => {
                *self.authenticated.lock() = true;
                Ok(())
            }
            Ok(Err(e)) => Err(ProtectError::Connect(format!("{addr}: {e}"))),
            Err(_) => Err(ProtectError::Timeout),
        }
    }

    /// Replace the NVR snapshot.
    pub fn set_nvr(&self, nvr: ProtectNvr) {
        *self.nvr.lock() = Some(nvr);
    }

    /// Borrow the current NVR snapshot, if any.
    #[must_use]
    pub fn nvr_snapshot(&self) -> Option<ProtectNvr> {
        self.nvr.lock().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c = NvrConfig::new("h", "u", "p");
        assert_eq!(c.port, 443);
        assert!(!c.verify_ssl);
        assert_eq!(c.max_media, 1000);
    }

    #[test]
    fn nvr_add_camera() {
        let mut n = ProtectNvr::new(NvrId::new("n"), "Ev NVR");
        n.add_camera(ProtectCamera::new(CameraId::new("c1"), "Salon"));
        assert_eq!(n.cameras.len(), 1);
    }

    #[test]
    fn client_unauthenticated_at_start() {
        let c = ProtectClient::new(NvrConfig::new("h", "u", "p"));
        assert!(!c.is_authenticated());
    }
}
