// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! Pairing — emulates the `POST /api` link-button flow.
//!
//! Reference:
//! - developers.meethue.com/develop/get-started-2 — "Press the link button on
//!   the bridge then POST { 'devicetype': 'app#user' } to /api within 30
//!   seconds to obtain a username".
//! - developers.meethue.com/develop/hue-api/7-configuration-api/#71_create_user
//!   — request/response schema for v1 user creation.
//! - developers.meethue.com/develop/hue-api-v2/api-reference/#auth_v1 — v2
//!   bridges accept the same POST plus an optional `generateclientkey`
//!   field for the Entertainment / clip-v2 application key.
//!
//! Charter v6 / ADR-007: the cave-home Portal admin module exposes a
//! "Bağlanmak için tuşa basın" UI (15s countdown). Underneath, the Portal
//! calls [`begin_link_window`] / [`is_link_button_pressed`] on this module.

use crate::errors::EmuError;
use parking_lot::Mutex;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the link button stays "pressed" after a Portal click.
/// Reference: developer-portal docs — 30 seconds.
pub const LINK_BUTTON_WINDOW: Duration = Duration::from_secs(30);

/// POST body for `POST /api`. Reference: v1 7.1 + v2 auth_v1 schemas.
#[derive(Debug, Clone, Deserialize)]
pub struct PairRequest {
    /// Device type / application identifier. Format `app_name#user` (max 40 chars).
    pub devicetype: String,
    /// v2 bridges: if true, also generate a `clientkey` for Entertainment.
    #[serde(default)]
    pub generateclientkey: bool,
}

/// One success entry in the `[{"success":{...}}]` response. Reference:
/// developer-portal v1 §7.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairSuccess {
    /// Granted application key (a.k.a. "username").
    pub username: String,
    /// Granted `clientkey` if requested. v2 only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clientkey: Option<String>,
}

/// A successfully whitelisted application key + when it was issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistEntry {
    pub app_key: String,
    pub device_type: String,
    /// ISO-like timestamp when the entry was created.
    pub create_date: String,
    /// Optional Entertainment clientkey (v2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_key: Option<String>,
}

/// Pairing state machine. Thread-safe; the HTTP layer + the Portal admin
/// both call into it.
#[derive(Debug, Clone, Default)]
pub struct PairingService {
    inner: Arc<Mutex<PairingInner>>,
}

#[derive(Debug, Default)]
struct PairingInner {
    /// When the link button was pressed (`None` => not pressed).
    button_pressed_at: Option<Instant>,
    /// `app_key -> entry`.
    whitelist: HashMap<String, WhitelistEntry>,
}

impl PairingService {
    /// Build a fresh service with an empty whitelist.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Portal-side "press the button". Starts a fresh 30-second window.
    pub fn begin_link_window(&self) {
        self.inner.lock().button_pressed_at = Some(Instant::now());
    }

    /// True iff the link button is currently in its open window.
    #[must_use]
    pub fn is_link_button_pressed(&self) -> bool {
        let guard = self.inner.lock();
        match guard.button_pressed_at {
            Some(t) => t.elapsed() < LINK_BUTTON_WINDOW,
            None => false,
        }
    }

    /// Try to pair. Returns the `PairSuccess` payload on success.
    pub fn try_pair(&self, req: &PairRequest, now: &str) -> Result<PairSuccess, EmuError> {
        if req.devicetype.is_empty() || req.devicetype.len() > 40 {
            return Err(EmuError::InvalidBody(format!(
                "devicetype length out of bounds (got {} chars, expected 1..=40)",
                req.devicetype.len()
            )));
        }
        let mut guard = self.inner.lock();
        let active = match guard.button_pressed_at {
            Some(t) => t.elapsed() < LINK_BUTTON_WINDOW,
            None => false,
        };
        if !active {
            return Err(EmuError::LinkButtonNotPressed);
        }
        let app_key = generate_app_key();
        let client_key = if req.generateclientkey {
            Some(generate_client_key())
        } else {
            None
        };
        guard.whitelist.insert(
            app_key.clone(),
            WhitelistEntry {
                app_key: app_key.clone(),
                device_type: req.devicetype.clone(),
                create_date: now.into(),
                client_key: client_key.clone(),
            },
        );
        Ok(PairSuccess {
            username: app_key,
            clientkey: client_key,
        })
    }

    /// Look up an application key.
    #[must_use]
    pub fn whitelist_get(&self, app_key: &str) -> Option<WhitelistEntry> {
        self.inner.lock().whitelist.get(app_key).cloned()
    }

    /// Snapshot of all entries (used by `/api/<appkey>/config.whitelist`).
    #[must_use]
    pub fn whitelist_all(&self) -> Vec<WhitelistEntry> {
        self.inner.lock().whitelist.values().cloned().collect()
    }

    /// Remove an entry (factory-reset / per-app deauth).
    pub fn whitelist_remove(&self, app_key: &str) -> bool {
        self.inner.lock().whitelist.remove(app_key).is_some()
    }
}

/// Generate a Hue-style application key. Reference: v1 docs - the bridge
/// returns a 32-character hex-ish string. We use 40 hex chars (matches
/// production-bridge observation noted in the public docs §7.1 examples).
fn generate_app_key() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 20] = std::array::from_fn(|_| rng.random::<u8>());
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a v2 Entertainment clientkey — 32 hex chars (per public docs).
fn generate_client_key() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 16] = std::array::from_fn(|_| rng.random::<u8>());
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_fails_without_button_press() {
        let svc = PairingService::new();
        let req = PairRequest {
            devicetype: "cave-home#user".into(),
            generateclientkey: false,
        };
        let err = svc.try_pair(&req, "2026-05-17T20:00:00").unwrap_err();
        assert!(matches!(err, EmuError::LinkButtonNotPressed));
    }

    #[test]
    fn pair_succeeds_within_window_and_whitelists() {
        let svc = PairingService::new();
        svc.begin_link_window();
        let req = PairRequest {
            devicetype: "cave-home#user".into(),
            generateclientkey: false,
        };
        let s = svc.try_pair(&req, "2026-05-17T20:00:00").unwrap();
        assert_eq!(s.username.len(), 40);
        assert!(s.clientkey.is_none());
        assert!(svc.whitelist_get(&s.username).is_some());
    }

    #[test]
    fn pair_with_generateclientkey_returns_clientkey() {
        let svc = PairingService::new();
        svc.begin_link_window();
        let req = PairRequest {
            devicetype: "cave-home#user".into(),
            generateclientkey: true,
        };
        let s = svc.try_pair(&req, "2026-05-17T20:00:00").unwrap();
        assert!(s.clientkey.is_some());
        assert_eq!(s.clientkey.as_ref().unwrap().len(), 32);
        let entry = svc.whitelist_get(&s.username).unwrap();
        assert!(entry.client_key.is_some());
    }

    #[test]
    fn devicetype_too_long_rejected() {
        let svc = PairingService::new();
        svc.begin_link_window();
        let req = PairRequest {
            devicetype: "x".repeat(41),
            generateclientkey: false,
        };
        let err = svc.try_pair(&req, "2026-05-17T20:00:00").unwrap_err();
        assert!(matches!(err, EmuError::InvalidBody(_)));
    }

    #[test]
    fn empty_devicetype_rejected() {
        let svc = PairingService::new();
        svc.begin_link_window();
        let req = PairRequest {
            devicetype: String::new(),
            generateclientkey: false,
        };
        let err = svc.try_pair(&req, "t").unwrap_err();
        assert!(matches!(err, EmuError::InvalidBody(_)));
    }

    #[test]
    fn whitelist_remove_works() {
        let svc = PairingService::new();
        svc.begin_link_window();
        let s = svc
            .try_pair(
                &PairRequest {
                    devicetype: "x#y".into(),
                    generateclientkey: false,
                },
                "t",
            )
            .unwrap();
        assert!(svc.whitelist_remove(&s.username));
        assert!(svc.whitelist_get(&s.username).is_none());
    }
}
