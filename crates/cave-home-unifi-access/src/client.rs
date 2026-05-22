// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi_access/__init__.py
//                            + unifi_access_api.UnifiAccessApiClient
//
// HA's __init__.py instantiates UnifiAccessApiClient(host, api_token, ...).
// Auth is by API token, not user/password — this is the key difference
// from unifi-network / unifi-protect.

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::door::{Door, DoorId, EmergencyStatus};
use crate::error::{AccessError, AccessResult};

/// UniFi Access connection config.
///
/// Source: HA `unifi_access/__init__.py` `async_setup_entry` —
/// reads `CONF_HOST`, `CONF_API_TOKEN`, `CONF_VERIFY_SSL`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessConfig {
    /// Hub hostname or IP.
    pub host: String,
    /// API token (UniFi Access settings → "Developer API").
    pub api_token: String,
    /// HTTPS port (UniFi Access defaults to 12445).
    pub port: u16,
    /// Verify the hub's TLS cert. Off by default since the hub ships
    /// with a self-signed cert.
    pub verify_ssl: bool,
}

impl AccessConfig {
    /// Construct a new config with sensible defaults.
    #[must_use]
    pub fn new(host: impl Into<String>, api_token: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            api_token: api_token.into(),
            port: 12445,
            verify_ssl: false,
        }
    }

    /// Override the port.
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
}

/// UniFi Access client.
///
/// Phase 1: validates the hub is reachable (TCP probe within HA's 10s
/// timeout). Phase 2 ticket: REST `/api/v2/developer/doors` crawl + WS
/// `/api/v1/developer/devices/notifications`.
pub struct AccessClient {
    cfg: AccessConfig,
    authenticated: Mutex<bool>,
    doors: Mutex<HashMap<DoorId, Door>>,
    emergency: Mutex<EmergencyStatus>,
}

impl AccessClient {
    /// Construct an unauthenticated client.
    #[must_use]
    pub fn new(cfg: AccessConfig) -> Self {
        Self {
            cfg,
            authenticated: Mutex::new(false),
            doors: Mutex::new(HashMap::new()),
            emergency: Mutex::new(EmergencyStatus::default()),
        }
    }

    /// Borrow the connection config.
    #[must_use]
    pub fn config(&self) -> &AccessConfig {
        &self.cfg
    }

    /// True if a successful `login()` has happened.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        *self.authenticated.lock()
    }

    /// Attempt to establish a session.
    pub async fn login(&mut self) -> AccessResult<()> {
        let addr = format!("{}:{}", self.cfg.host, self.cfg.port);
        match timeout(Duration::from_secs(10), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => {
                *self.authenticated.lock() = true;
                Ok(())
            }
            Ok(Err(e)) => Err(AccessError::Connect(format!("{addr}: {e}"))),
            Err(_) => Err(AccessError::Timeout),
        }
    }

    /// Insert / update a door in the cache.
    pub fn upsert_door(&self, door: Door) {
        self.doors.lock().insert(door.id.clone(), door);
    }

    /// Look up a door by ID.
    #[must_use]
    pub fn get_door(&self, id: &DoorId) -> Option<Door> {
        self.doors.lock().get(id).cloned()
    }

    /// Snapshot every door.
    #[must_use]
    pub fn doors_snapshot(&self) -> Vec<Door> {
        self.doors.lock().values().cloned().collect()
    }

    /// Count cached doors.
    #[must_use]
    pub fn door_count(&self) -> usize {
        self.doors.lock().len()
    }

    /// Update the global emergency status.
    pub fn set_emergency(&self, status: EmergencyStatus) {
        *self.emergency.lock() = status;
    }

    /// Read the current emergency status.
    #[must_use]
    pub fn emergency(&self) -> EmergencyStatus {
        self.emergency.lock().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_is_12445() {
        let c = AccessConfig::new("h", "tok");
        assert_eq!(c.port, 12445);
        assert!(!c.verify_ssl);
    }

    #[test]
    fn unauthenticated_on_construct() {
        let c = AccessClient::new(AccessConfig::new("h", "tok"));
        assert!(!c.is_authenticated());
    }

    #[test]
    fn upsert_and_count() {
        let c = AccessClient::new(AccessConfig::new("h", "tok"));
        c.upsert_door(Door::new(DoorId::new("d1"), "Ön"));
        c.upsert_door(Door::new(DoorId::new("d2"), "Salon"));
        assert_eq!(c.door_count(), 2);
        let d = c.get_door(&DoorId::new("d1")).unwrap();
        assert_eq!(d.label, "Ön");
    }
}
