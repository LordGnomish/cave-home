// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/__init__.py
//                            (UnifiWirelessClients) +
//                            aiounifi/models/client.py (Client model).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::identifiers::ClientId;

/// A client device tracked by the UniFi controller.
///
/// HA model (`aiounifi.models.client.Client`) is wide (mac, hostname,
/// ip, vlan, ap_mac, last_seen, is_wired, ...). cave-home Phase 1
/// surfaces the four fields the portal needs; Phase 2 ticket: full
/// `Client` parity behind a feature flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiClient {
    /// MAC-keyed client ID.
    pub id: ClientId,
    /// Friendly label (hostname or controller-set name).
    pub label: String,
    /// True if connected via Ethernet (HA `Client.is_wired`).
    pub is_wired: bool,
    /// True if a `block_client` switch has been flipped on for this MAC.
    pub blocked: bool,
}

impl UnifiClient {
    /// Construct a new client entry.
    #[must_use]
    pub fn new(id: ClientId, label: impl Into<String>, is_wired: bool) -> Self {
        Self {
            id,
            label: label.into(),
            is_wired,
            blocked: false,
        }
    }
}

/// Persistent "known-wireless" registry.
///
/// Source: HA `UnifiWirelessClients` class in `__init__.py`. UniFi
/// marks a wireless client as wired once it goes offline; HA
/// remembers the wireless ones so they keep tracking correctly.
/// cave-home replicates the same in-memory semantics; persistence to
/// disk is a Phase 2 ticket.
#[derive(Default, Debug)]
pub struct WirelessClientRegistry {
    known_wireless: HashSet<ClientId>,
}

impl WirelessClientRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True if this client has ever been seen wireless.
    /// Mirrors HA `UnifiWirelessClients.is_wireless(client)`: a side
    /// effect — calling on a non-wired previously-unknown client
    /// records it.
    pub fn is_wireless(&mut self, client: &UnifiClient) -> bool {
        if !client.is_wired && !self.known_wireless.contains(&client.id) {
            self.known_wireless.insert(client.id.clone());
        }
        self.known_wireless.contains(&client.id)
    }

    /// Update from a batch of currently-seen clients.
    pub fn update<'a>(&mut self, clients: impl IntoIterator<Item = &'a UnifiClient>) {
        for c in clients {
            if !c.is_wired {
                self.known_wireless.insert(c.id.clone());
            }
        }
    }

    /// Count of clients tracked as wireless.
    #[must_use]
    pub fn len(&self) -> usize {
        self.known_wireless.len()
    }

    /// True if zero wireless clients are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.known_wireless.is_empty()
    }

    /// Check membership.
    #[must_use]
    pub fn contains(&self, id: &ClientId) -> bool {
        self.known_wireless.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wireless_registry_records_on_first_sight() {
        let mut r = WirelessClientRegistry::new();
        let c = UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:ff"), "iPhone", false);
        assert!(!r.contains(&c.id));
        assert!(r.is_wireless(&c));
        assert!(r.contains(&c.id));
    }

    #[test]
    fn wireless_registry_ignores_wired() {
        let mut r = WirelessClientRegistry::new();
        let c = UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:00"), "Desktop", true);
        assert!(!r.is_wireless(&c));
        assert!(!r.contains(&c.id));
    }

    #[test]
    fn registry_update_batch() {
        let mut r = WirelessClientRegistry::new();
        let a = UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:01"), "A", false);
        let b = UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:02"), "B", true);
        let c = UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:03"), "C", false);
        r.update([&a, &b, &c]);
        assert_eq!(r.len(), 2);
        assert!(r.contains(&a.id));
        assert!(!r.contains(&b.id));
        assert!(r.contains(&c.id));
    }
}
