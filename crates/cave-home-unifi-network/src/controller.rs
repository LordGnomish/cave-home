// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/hub/api.py +
//                            homeassistant/components/unifi/hub/hub.py
//
// HA's `UnifiHub` is the runtime object that holds the aiounifi
// `Controller` plus the entity / WebSocket / device registry caches.
// cave-home Phase 1 ports the connect / login surface and the in-memory
// registries; the wire protocol (REST + WS) is stubbed behind a
// feature flag the binary doesn't yet enable. Phase 2 ticket:
// real-network smoke test against a UDM-Pro.

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::client::{UnifiClient, WirelessClientRegistry};
use crate::const_table::{
    DEFAULT_ALLOW_BANDWIDTH_SENSORS, DEFAULT_ALLOW_UPTIME_SENSORS, DEFAULT_DETECTION_TIME_SECS,
    DEFAULT_DPI_RESTRICTIONS, DEFAULT_IGNORE_WIRED_BUG, DEFAULT_TRACK_CLIENTS,
    DEFAULT_TRACK_DEVICES, DEFAULT_TRACK_WIRED_CLIENTS,
};
use crate::device::UnifiDevice;
use crate::error::{UnifiError, UnifiResult};
use crate::identifiers::{ClientId, DeviceId, SiteId};

/// UniFi controller connection config.
///
/// Source: HA `homeassistant/components/unifi/hub/api.py`
/// `get_unifi_api()` reads `CONF_HOST`, `CONF_USERNAME`, `CONF_PASSWORD`,
/// `CONF_PORT`, `CONF_SITE_ID`, `CONF_VERIFY_SSL` from a `Mapping`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerConfig {
    /// Controller hostname or IP.
    pub host: String,
    /// Login username (admin or local-only user).
    pub username: String,
    /// Login password.
    pub password: String,
    /// HTTPS port — defaults to 8443 (UniFi Network v8 self-hosted).
    /// UDM / Cloud Gateway uses 443; user must override.
    pub port: u16,
    /// Site identifier.
    pub site: SiteId,
    /// Whether to verify the controller's TLS certificate.
    pub verify_ssl: bool,
    /// Track clients (HA `CONF_TRACK_CLIENTS`).
    pub track_clients: bool,
    /// Track devices (HA `CONF_TRACK_DEVICES`).
    pub track_devices: bool,
    /// Track wired clients (HA `CONF_TRACK_WIRED_CLIENTS`).
    pub track_wired_clients: bool,
    /// Surface per-client bandwidth sensors.
    pub allow_bandwidth_sensors: bool,
    /// Surface per-client uptime sensors.
    pub allow_uptime_sensors: bool,
    /// Enforce DPI restrictions.
    pub dpi_restrictions: bool,
    /// Ignore the wired-bug heuristic.
    pub ignore_wired_bug: bool,
    /// Detection-time threshold in seconds.
    pub detection_time_secs: u32,
}

impl ControllerConfig {
    /// Construct a config with the HA defaults applied.
    #[must_use]
    pub fn new(host: impl Into<String>, username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            username: username.into(),
            password: password.into(),
            port: 8443,
            site: SiteId::default(),
            verify_ssl: true,
            track_clients: DEFAULT_TRACK_CLIENTS,
            track_devices: DEFAULT_TRACK_DEVICES,
            track_wired_clients: DEFAULT_TRACK_WIRED_CLIENTS,
            allow_bandwidth_sensors: DEFAULT_ALLOW_BANDWIDTH_SENSORS,
            allow_uptime_sensors: DEFAULT_ALLOW_UPTIME_SENSORS,
            dpi_restrictions: DEFAULT_DPI_RESTRICTIONS,
            ignore_wired_bug: DEFAULT_IGNORE_WIRED_BUG,
            detection_time_secs: DEFAULT_DETECTION_TIME_SECS,
        }
    }

    /// Override the controller port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Override the site.
    #[must_use]
    pub fn with_site(mut self, site: impl Into<String>) -> Self {
        self.site = SiteId::new(site);
        self
    }

    /// Override the TLS verify flag (disable for self-signed
    /// controllers).
    #[must_use]
    pub fn with_verify_ssl(mut self, verify: bool) -> Self {
        self.verify_ssl = verify;
        self
    }
}

/// In-memory controller cache + login state machine.
///
/// HA's `UnifiHub` mounts a long-lived `aiounifi.Controller`; the
/// hub keeps the `clients` / `devices` dicts populated and runs the
/// WebSocket task. cave-home Phase 1 mirrors the data-flow shape:
/// `login()` validates the host with a TCP connect probe; full REST
/// crawl is the Phase 2 wire-up.
pub struct UnifiController {
    cfg: ControllerConfig,
    authenticated: parking_lot::Mutex<bool>,
    clients: RwLock<HashMap<ClientId, UnifiClient>>,
    devices: RwLock<HashMap<DeviceId, UnifiDevice>>,
    wireless: parking_lot::Mutex<WirelessClientRegistry>,
}

impl UnifiController {
    /// Build a controller bound to the given config. Login is lazy;
    /// call `login()` to establish the session.
    #[must_use]
    pub fn new(cfg: ControllerConfig) -> Self {
        Self {
            cfg,
            authenticated: parking_lot::Mutex::new(false),
            clients: RwLock::new(HashMap::new()),
            devices: RwLock::new(HashMap::new()),
            wireless: parking_lot::Mutex::new(WirelessClientRegistry::new()),
        }
    }

    /// Borrow the controller config.
    #[must_use]
    pub fn config(&self) -> &ControllerConfig {
        &self.cfg
    }

    /// True if a successful `login()` has happened on this instance.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        *self.authenticated.lock()
    }

    /// Attempt to establish a session.
    ///
    /// Phase 1 implementation: TCP-probe the controller `host:port`
    /// with a 10-second timeout (mirrors HA's `asyncio.timeout(10)`
    /// wrapper around `api.login()`). A successful probe sets the
    /// authenticated flag. The full REST `api/login` POST is the
    /// Phase 2 wire-up; the flag's contract is "did we get to the
    /// controller". Phase 2 ticket: real `POST /api/login` + cookie.
    pub async fn login(&mut self) -> UnifiResult<()> {
        let addr = format!("{}:{}", self.cfg.host, self.cfg.port);
        let result = timeout(Duration::from_secs(10), TcpStream::connect(&addr)).await;
        match result {
            Ok(Ok(_)) => {
                *self.authenticated.lock() = true;
                Ok(())
            }
            Ok(Err(e)) => Err(UnifiError::Connect(format!("{addr}: {e}"))),
            Err(_) => Err(UnifiError::Timeout),
        }
    }

    /// Insert / update a known client.
    pub fn upsert_client(&self, c: UnifiClient) {
        let mut w = self.wireless.lock();
        w.is_wireless(&c);
        drop(w);
        self.clients.write().insert(c.id.clone(), c);
    }

    /// Insert / update a known device.
    pub fn upsert_device(&self, d: UnifiDevice) {
        self.devices.write().insert(d.id.clone(), d);
    }

    /// Count tracked clients.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.clients.read().len()
    }

    /// Count tracked devices.
    #[must_use]
    pub fn device_count(&self) -> usize {
        self.devices.read().len()
    }

    /// Snapshot all clients.
    #[must_use]
    pub fn clients_snapshot(&self) -> Vec<UnifiClient> {
        self.clients.read().values().cloned().collect()
    }

    /// Snapshot all devices.
    #[must_use]
    pub fn devices_snapshot(&self) -> Vec<UnifiDevice> {
        self.devices.read().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceKind;

    #[test]
    fn config_defaults() {
        let c = ControllerConfig::new("h", "u", "p");
        assert_eq!(c.port, 8443);
        assert!(c.verify_ssl);
        assert_eq!(c.site.as_str(), "default");
        assert_eq!(c.detection_time_secs, 300);
    }

    #[test]
    fn controller_starts_unauthenticated() {
        let c = UnifiController::new(ControllerConfig::new("h", "u", "p"));
        assert!(!c.is_authenticated());
    }

    #[test]
    fn upsert_client_and_device_count() {
        let c = UnifiController::new(ControllerConfig::new("h", "u", "p"));
        c.upsert_client(UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:01"), "A", false));
        c.upsert_client(UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:02"), "B", true));
        c.upsert_device(UnifiDevice::new(
            DeviceId::new("aa:bb:cc:dd:ee:f0"),
            "sw",
            DeviceKind::Switch,
        ));
        assert_eq!(c.client_count(), 2);
        assert_eq!(c.device_count(), 1);
    }
}
