// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// UniFi Talk roster model. The Ubiquiti Talk public REST surface
// exposes a phone list via `/api/talk/phones` (subject to change —
// Ubiquiti has not stabilised this endpoint). cave-home stores the
// roster in memory and refreshes from the API when available.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Unique phone identifier (UniFi Talk phone GUID).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhoneId(String);

impl PhoneId {
    /// Construct from any string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }
    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PhoneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One UniFi TalkPhone (hardware or softphone).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalkPhone {
    /// Phone GUID.
    pub id: PhoneId,
    /// User-set label.
    pub label: String,
    /// Assigned extension number / E.164 string.
    pub extension: String,
    /// True if this phone is currently on a call.
    pub is_busy: bool,
}

impl TalkPhone {
    /// Construct a free / idle phone.
    #[must_use]
    pub fn new(
        id: PhoneId,
        label: impl Into<String>,
        extension: impl Into<String>,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            extension: extension.into(),
            is_busy: false,
        }
    }
}

/// Phone roster — in-memory collection of every phone known to the
/// Talk hub.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PhoneRoster {
    phones: HashMap<PhoneId, TalkPhone>,
}

impl PhoneRoster {
    /// Construct an empty roster.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a phone.
    pub fn add(&mut self, phone: TalkPhone) {
        self.phones.insert(phone.id.clone(), phone);
    }

    /// Look up a phone by ID.
    #[must_use]
    pub fn get(&self, id: &PhoneId) -> Option<&TalkPhone> {
        self.phones.get(id)
    }

    /// Remove a phone.
    pub fn remove(&mut self, id: &PhoneId) -> Option<TalkPhone> {
        self.phones.remove(id)
    }

    /// Count phones in the roster.
    #[must_use]
    pub fn len(&self) -> usize {
        self.phones.len()
    }

    /// True if no phones tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.phones.is_empty()
    }

    /// Iterate every phone.
    pub fn iter(&self) -> impl Iterator<Item = &TalkPhone> {
        self.phones.values()
    }
}

/// ADR-007 home-world phone label.
#[must_use]
pub fn friendly_phone_label(user_name: &str) -> String {
    let trimmed = user_name.trim();
    if trimmed.is_empty() {
        "Adsız interkom".to_string()
    } else {
        format!("{trimmed} interkomu")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_add_get_remove() {
        let mut r = PhoneRoster::new();
        r.add(TalkPhone::new(PhoneId::new("p1"), "Mutfak", "100"));
        assert_eq!(r.len(), 1);
        assert_eq!(r.get(&PhoneId::new("p1")).unwrap().extension, "100");
        let removed = r.remove(&PhoneId::new("p1"));
        assert!(removed.is_some());
        assert!(r.is_empty());
    }

    #[test]
    fn friendly_label() {
        assert_eq!(friendly_phone_label("Salon"), "Salon interkomu");
        assert_eq!(friendly_phone_label(""), "Adsız interkom");
    }
}
