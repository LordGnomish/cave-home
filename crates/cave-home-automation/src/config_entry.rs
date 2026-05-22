// SPDX-License-Identifier: Apache-2.0
//! Config entries / flow scaffold — port of
//! `homeassistant/config_entries.py` (data model + flow handler API
//! only; the full Python class is 4 kLOC and most of it is HTTP
//! plumbing that lives in cave-home-portal in our port).
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/config_entries.py

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ulid::Ulid;

use crate::error::{HassError, HassResult};

/// Lifecycle states of a [`ConfigEntry`].
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/config_entries.py::ConfigEntryState
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigEntryState {
    /// The config entry has not been loaded yet.
    NotLoaded,
    /// The config entry has been loaded.
    Loaded,
    /// The config entry has been unloaded.
    Unloaded,
    /// The config entry has been disabled by the user.
    Disabled,
    /// Setup of the entry errored.
    SetupError,
    /// Migration of the entry failed.
    MigrationError,
}

/// Source of a config entry — mirrors HA's `SOURCE_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigEntrySource {
    User,
    Import,
    Reauth,
    Reconfigure,
    Discovery,
}

/// A single config entry — a configured instance of an integration.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/config_entries.py::ConfigEntry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub entry_id: String,
    pub domain: String,
    pub title: String,
    pub data: Value,
    pub options: Value,
    pub source: ConfigEntrySource,
    pub state: ConfigEntryState,
    /// Schema version; incremented when the entry's `data` shape changes.
    pub version: u32,
}

impl ConfigEntry {
    /// New entry with a freshly-generated ULID id.
    #[must_use]
    pub fn new(
        domain: impl Into<String>,
        title: impl Into<String>,
        data: Value,
        source: ConfigEntrySource,
    ) -> Self {
        Self {
            entry_id: Ulid::new().to_string(),
            domain: domain.into(),
            title: title.into(),
            data,
            options: Value::Object(serde_json::Map::new()),
            source,
            state: ConfigEntryState::NotLoaded,
            version: 1,
        }
    }
}

/// A step in a config flow returned by [`ConfigFlowHandler::step`].
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/data_entry_flow.py::FlowResult
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowResult {
    /// The flow needs more input.
    Form {
        step_id: String,
        schema_hint: Value,
        errors: HashMap<String, String>,
    },
    /// The flow completed successfully, producing a new entry.
    Create { entry: ConfigEntry },
    /// The flow aborted.
    Abort { reason: String },
}

/// Trait implemented by per-integration config flow handlers.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/config_entries.py::ConfigFlow
#[async_trait]
pub trait ConfigFlowHandler: Send + Sync {
    /// Domain this flow handles — must match the eventual entry's `domain`.
    fn domain(&self) -> &str;

    /// Run a single step of the flow.
    ///
    /// `step_id` selects the step ("user", "discovery_confirm", ...);
    /// `user_input` carries form data (may be `None` on first invocation).
    async fn step(
        &self,
        step_id: &str,
        user_input: Option<Value>,
    ) -> HassResult<FlowResult>;
}

/// The collection of all known config entries.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/config_entries.py::ConfigEntries
#[derive(Debug, Default)]
pub struct ConfigEntries {
    by_id: RwLock<HashMap<String, ConfigEntry>>,
    by_domain: RwLock<HashMap<String, Vec<String>>>,
}

impl ConfigEntries {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new entry.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/config_entries.py::ConfigEntries.async_add
    pub fn add(&self, entry: ConfigEntry) {
        self.by_domain
            .write()
            .entry(entry.domain.clone())
            .or_default()
            .push(entry.entry_id.clone());
        self.by_id.write().insert(entry.entry_id.clone(), entry);
    }

    /// Update an existing entry's state.
    pub fn set_state(&self, entry_id: &str, state: ConfigEntryState) -> HassResult<()> {
        let mut g = self.by_id.write();
        let e = g
            .get_mut(entry_id)
            .ok_or_else(|| HassError::UnknownConfigEntry(entry_id.to_owned()))?;
        e.state = state;
        Ok(())
    }

    /// Lookup an entry.
    pub fn get(&self, entry_id: &str) -> Option<ConfigEntry> {
        self.by_id.read().get(entry_id).cloned()
    }

    /// List entries for a given domain.
    pub fn for_domain(&self, domain: &str) -> Vec<ConfigEntry> {
        let by_id = self.by_id.read();
        self.by_domain
            .read()
            .get(domain)
            .map(|ids| ids.iter().filter_map(|i| by_id.get(i).cloned()).collect())
            .unwrap_or_default()
    }

    /// All entries.
    pub fn all(&self) -> Vec<ConfigEntry> {
        self.by_id.read().values().cloned().collect()
    }

    /// Remove an entry.
    pub fn remove(&self, entry_id: &str) -> Option<ConfigEntry> {
        let entry = self.by_id.write().remove(entry_id)?;
        if let Some(vec) = self.by_domain.write().get_mut(&entry.domain) {
            vec.retain(|id| id != entry_id);
        }
        Some(entry)
    }
}

/// Registry of flow handlers, keyed by integration domain.
#[derive(Default)]
pub struct ConfigFlowRegistry {
    handlers: RwLock<HashMap<String, Arc<dyn ConfigFlowHandler>>>,
}

impl std::fmt::Debug for ConfigFlowRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.handlers.read();
        f.debug_struct("ConfigFlowRegistry")
            .field("domains", &g.keys().cloned().collect::<Vec<_>>())
            .finish()
    }
}

impl ConfigFlowRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, handler: Arc<dyn ConfigFlowHandler>) {
        let key = handler.domain().to_owned();
        self.handlers.write().insert(key, handler);
    }

    #[must_use]
    pub fn get(&self, domain: &str) -> Option<Arc<dyn ConfigFlowHandler>> {
        self.handlers.read().get(domain).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream-test: `tests/test_config_entries.py::test_call_async_migrate_entry`
    #[test]
    fn config_entry_lifecycle() {
        let entries = ConfigEntries::new();
        let entry = ConfigEntry::new(
            "hue",
            "Hue Bridge",
            serde_json::json!({"host": "10.0.0.5"}),
            ConfigEntrySource::User,
        );
        let id = entry.entry_id.clone();
        entries.add(entry);
        assert_eq!(entries.for_domain("hue").len(), 1);
        entries.set_state(&id, ConfigEntryState::Loaded).unwrap();
        let stored = entries.get(&id).unwrap();
        assert_eq!(stored.state, ConfigEntryState::Loaded);
        assert!(entries.remove(&id).is_some());
        assert!(entries.get(&id).is_none());
    }

    struct DummyFlow;

    #[async_trait]
    impl ConfigFlowHandler for DummyFlow {
        fn domain(&self) -> &str {
            "hue"
        }

        async fn step(
            &self,
            step_id: &str,
            user_input: Option<Value>,
        ) -> HassResult<FlowResult> {
            if step_id == "user" && user_input.is_none() {
                return Ok(FlowResult::Form {
                    step_id: "user".into(),
                    schema_hint: serde_json::json!({"host": "string"}),
                    errors: HashMap::new(),
                });
            }
            let entry = ConfigEntry::new(
                "hue",
                "Hue",
                user_input.unwrap_or(Value::Null),
                ConfigEntrySource::User,
            );
            Ok(FlowResult::Create { entry })
        }
    }

    #[tokio::test]
    async fn flow_handler_two_step_creates_entry() {
        let reg = ConfigFlowRegistry::new();
        reg.register(Arc::new(DummyFlow));
        let handler = reg.get("hue").unwrap();
        match handler.step("user", None).await.unwrap() {
            FlowResult::Form { step_id, .. } => assert_eq!(step_id, "user"),
            _ => panic!("expected form"),
        }
        match handler
            .step("user", Some(serde_json::json!({"host": "1.1.1.1"})))
            .await
            .unwrap()
        {
            FlowResult::Create { entry } => assert_eq!(entry.domain, "hue"),
            _ => panic!("expected create"),
        }
    }
}
