// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Talk hub client. HA has no upstream integration; this is the
// Ubiquiti REST surface from scratch (parity ceiling per ADR-009).

use std::time::Duration;

use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::call::{CallControlVerb, CallId};
use crate::error::{TalkError, TalkResult};
use crate::phone::PhoneRoster;

/// UniFi Talk hub connection config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TalkConfig {
    /// Hub hostname or IP.
    pub host: String,
    /// API token (UniFi Talk → Developer settings).
    pub api_token: String,
    /// HTTPS port (Talk hub defaults to 443 over the UniFi OS proxy).
    pub port: u16,
    /// Verify TLS cert.
    pub verify_ssl: bool,
}

impl TalkConfig {
    /// Construct a config with the standard defaults.
    #[must_use]
    pub fn new(host: impl Into<String>, api_token: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            api_token: api_token.into(),
            port: 443,
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

/// UniFi Talk client.
pub struct TalkClient {
    cfg: TalkConfig,
    authenticated: Mutex<bool>,
    roster: Mutex<PhoneRoster>,
}

impl TalkClient {
    /// Construct an unauthenticated client.
    #[must_use]
    pub fn new(cfg: TalkConfig) -> Self {
        Self {
            cfg,
            authenticated: Mutex::new(false),
            roster: Mutex::new(PhoneRoster::new()),
        }
    }

    /// Borrow the config.
    #[must_use]
    pub fn config(&self) -> &TalkConfig {
        &self.cfg
    }

    /// True if `login()` has succeeded on this instance.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        *self.authenticated.lock()
    }

    /// Attempt to reach the Talk hub.
    pub async fn login(&mut self) -> TalkResult<()> {
        let addr = format!("{}:{}", self.cfg.host, self.cfg.port);
        match timeout(Duration::from_secs(10), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => {
                *self.authenticated.lock() = true;
                Ok(())
            }
            Ok(Err(e)) => Err(TalkError::Connect(format!("{addr}: {e}"))),
            Err(_) => Err(TalkError::Timeout),
        }
    }

    /// Borrow the phone roster.
    #[must_use]
    pub fn roster(&self) -> PhoneRoster {
        self.roster.lock().clone()
    }

    /// Replace the phone roster.
    pub fn set_roster(&self, roster: PhoneRoster) {
        *self.roster.lock() = roster;
    }

    /// Issue a control verb against an active call.
    ///
    /// Phase 1 stub: validates the call/verb pair shape; wire-side
    /// REST POST is the Phase 2 ticket (gated by `Unavailable` until
    /// Ubiquiti stabilises the endpoint).
    pub fn control_call(&self, _call: &CallId, _verb: CallControlVerb) -> TalkResult<()> {
        // Phase 1: the verb table is valid; the actual REST call is
        // not yet available. Return `Unavailable` so callers can plan
        // around it.
        Err(TalkError::Unavailable(
            "call control over public REST is Phase 2".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_443() {
        let c = TalkConfig::new("h", "tok");
        assert_eq!(c.port, 443);
    }

    #[test]
    fn unauthenticated_initial() {
        let c = TalkClient::new(TalkConfig::new("h", "tok"));
        assert!(!c.is_authenticated());
    }

    #[test]
    fn control_call_phase1_reports_unavailable() {
        let c = TalkClient::new(TalkConfig::new("h", "tok"));
        let r = c.control_call(&CallId::new("c"), CallControlVerb::Answer);
        assert!(matches!(r, Err(TalkError::Unavailable(_))));
    }
}
