//! Port of `homeassistant.helpers.entity` — the `Entity` base class and its
//! `DeviceInfo` / `EntityCategory` companions.
//!
//! In HA every integration platform produces `Entity` subclasses. The base
//! class is mostly a bag of overridable *properties* (`state`, `name`,
//! `available`, `device_class`, …) plus the machinery that snapshots those
//! properties into a [`State`](crate::state::State) and writes it to the
//! [`StateMachine`](crate::state_machine::StateMachine). This module ports the
//! property surface as a Rust trait with the same HA defaults, and the
//! snapshot step as [`Entity::state_snapshot`].

use crate::state::StateAttributes;
use serde::{Deserialize, Serialize};

/// `homeassistant.const.STATE_UNAVAILABLE`.
pub const STATE_UNAVAILABLE: &str = "unavailable";
/// `homeassistant.const.STATE_UNKNOWN`.
pub const STATE_UNKNOWN: &str = "unknown";

// Standard attribute keys HA folds into every state snapshot
// (`homeassistant.const.ATTR_*`).
pub const ATTR_FRIENDLY_NAME: &str = "friendly_name";
pub const ATTR_DEVICE_CLASS: &str = "device_class";
pub const ATTR_ICON: &str = "icon";
pub const ATTR_SUPPORTED_FEATURES: &str = "supported_features";
pub const ATTR_ASSUMED_STATE: &str = "assumed_state";
pub const ATTR_ATTRIBUTION: &str = "attribution";

/// Port of `homeassistant.helpers.entity.EntityCategory`.
///
/// Classifies an entity as configuration- or diagnostic-only so the frontend
/// can file it away from the primary controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityCategory {
    #[serde(rename = "config")]
    Config,
    #[serde(rename = "diagnostic")]
    Diagnostic,
}

/// Port of `homeassistant.helpers.entity.DeviceInfo`.
///
/// The hint an entity hands the device registry so its physical device can be
/// created or linked. `identifiers` are integration-scoped `(domain, id)`
/// tuples; `connections` are hardware addresses such as `(mac, …)`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    #[serde(default)]
    pub identifiers: Vec<(String, String)>,
    #[serde(default)]
    pub connections: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hw_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    /// `(domain, id)` of the hub/bridge this device is reached through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_device: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_url: Option<String>,
}

/// Port of the property surface of `homeassistant.helpers.entity.Entity`.
///
/// Defaults mirror upstream: `available` is `true`, `should_poll` is `true`,
/// `assumed_state` is `false`, optional metadata is `None`, attribute bags are
/// empty. Integrations override only what they need.
pub trait Entity {
    /// `Entity.unique_id` — stable id used by the entity registry. `None`
    /// means the entity is not registry-tracked.
    fn unique_id(&self) -> Option<String> {
        None
    }

    /// `Entity.name` — the entity's own name (folded into `friendly_name`).
    fn name(&self) -> Option<String> {
        None
    }

    /// `Entity.state` — the textual state value. `None` becomes
    /// [`STATE_UNKNOWN`] in the snapshot.
    fn state(&self) -> Option<String>;

    /// `Entity.extra_state_attributes` — integration-specific attributes.
    fn extra_state_attributes(&self) -> StateAttributes {
        StateAttributes::new()
    }

    /// `Entity.capability_attributes` — static capability descriptors
    /// (e.g. a light's `supported_color_modes`).
    fn capability_attributes(&self) -> StateAttributes {
        StateAttributes::new()
    }

    /// `Entity.device_class`.
    fn device_class(&self) -> Option<String> {
        None
    }

    /// `Entity.entity_category`.
    fn entity_category(&self) -> Option<EntityCategory> {
        None
    }

    /// `Entity.icon` (mdi key).
    fn icon(&self) -> Option<String> {
        None
    }

    /// `Entity.attribution`.
    fn attribution(&self) -> Option<String> {
        None
    }

    /// `Entity.supported_features` bitmask. `0` means none.
    fn supported_features(&self) -> u32 {
        0
    }

    /// `Entity.available` — `false` forces the state to [`STATE_UNAVAILABLE`].
    fn available(&self) -> bool {
        true
    }

    /// `Entity.should_poll`.
    fn should_poll(&self) -> bool {
        true
    }

    /// `Entity.assumed_state` — `true` when the integration cannot read back
    /// the real device state (folded into the `assumed_state` attribute).
    fn assumed_state(&self) -> bool {
        false
    }

    /// `Entity.device_info` — hint for the device registry.
    fn device_info(&self) -> Option<DeviceInfo> {
        None
    }

    /// Snapshot this entity into the `(state, attributes)` pair the
    /// [`StateMachine`](crate::state_machine::StateMachine) stores, applying
    /// HA's `_async_write_ha_state` folding rules:
    ///
    /// * unavailable → state is [`STATE_UNAVAILABLE`] with no attributes;
    /// * `state()` of `None` → [`STATE_UNKNOWN`];
    /// * capability + extra attributes are merged, then the standard
    ///   `friendly_name` / `device_class` / `icon` / `supported_features` /
    ///   `assumed_state` / `attribution` keys are added when present.
    fn state_snapshot(&self) -> (String, StateAttributes) {
        // Unavailable short-circuits: HA writes STATE_UNAVAILABLE with no
        // attributes regardless of what the property getters return.
        if !self.available() {
            return (STATE_UNAVAILABLE.to_owned(), StateAttributes::new());
        }

        let state = self.state().unwrap_or_else(|| STATE_UNKNOWN.to_owned());

        // capability attributes form the base; extra (dynamic) attributes are
        // layered on top so an integration can shadow a capability key.
        let mut attrs = self.capability_attributes();
        attrs.extend(self.extra_state_attributes());

        // Standard keys, added only when meaningful (mirrors upstream, which
        // omits absent metadata and the zero/false defaults).
        if let Some(name) = self.name() {
            attrs.insert(ATTR_FRIENDLY_NAME.into(), serde_json::Value::String(name));
        }
        if let Some(dc) = self.device_class() {
            attrs.insert(ATTR_DEVICE_CLASS.into(), serde_json::Value::String(dc));
        }
        if let Some(icon) = self.icon() {
            attrs.insert(ATTR_ICON.into(), serde_json::Value::String(icon));
        }
        if let Some(attribution) = self.attribution() {
            attrs.insert(
                ATTR_ATTRIBUTION.into(),
                serde_json::Value::String(attribution),
            );
        }
        let features = self.supported_features();
        if features != 0 {
            attrs.insert(ATTR_SUPPORTED_FEATURES.into(), features.into());
        }
        if self.assumed_state() {
            attrs.insert(ATTR_ASSUMED_STATE.into(), serde_json::Value::Bool(true));
        }

        (state, attrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A minimal light that overrides only what it needs — exercises the
    /// trait defaults for everything else.
    struct Light {
        on: bool,
        brightness: u8,
        reachable: bool,
    }

    impl Entity for Light {
        fn unique_id(&self) -> Option<String> {
            Some("light_kitchen".into())
        }
        fn name(&self) -> Option<String> {
            Some("Kitchen".into())
        }
        fn state(&self) -> Option<String> {
            Some(if self.on { "on" } else { "off" }.into())
        }
        fn extra_state_attributes(&self) -> StateAttributes {
            let mut a = StateAttributes::new();
            a.insert("brightness".into(), json!(self.brightness));
            a
        }
        fn capability_attributes(&self) -> StateAttributes {
            let mut a = StateAttributes::new();
            a.insert("supported_color_modes".into(), json!(["brightness"]));
            a
        }
        fn device_class(&self) -> Option<String> {
            None
        }
        fn supported_features(&self) -> u32 {
            0b0000_0001
        }
        fn available(&self) -> bool {
            self.reachable
        }
    }

    #[test]
    fn trait_defaults_match_upstream() {
        struct Bare;
        impl Entity for Bare {
            fn state(&self) -> Option<String> {
                None
            }
        }
        let b = Bare;
        assert!(b.available());
        assert!(b.should_poll());
        assert!(!b.assumed_state());
        assert_eq!(b.supported_features(), 0);
        assert!(b.unique_id().is_none());
        assert!(b.device_info().is_none());
        assert!(b.entity_category().is_none());
        assert!(b.capability_attributes().is_empty());
    }

    #[test]
    fn snapshot_folds_friendly_name_and_standard_attrs() {
        let l = Light { on: true, brightness: 200, reachable: true };
        let (state, attrs) = l.state_snapshot();
        assert_eq!(state, "on");
        // friendly_name comes from name()
        assert_eq!(attrs[ATTR_FRIENDLY_NAME], json!("Kitchen"));
        // capability + extra are both merged
        assert_eq!(attrs["brightness"], json!(200));
        assert_eq!(attrs["supported_color_modes"], json!(["brightness"]));
        // non-zero supported_features is folded in
        assert_eq!(attrs[ATTR_SUPPORTED_FEATURES], json!(1));
        // device_class is absent (None) → key not present
        assert!(!attrs.contains_key(ATTR_DEVICE_CLASS));
        // assumed_state false → key omitted (HA only adds it when true)
        assert!(!attrs.contains_key(ATTR_ASSUMED_STATE));
    }

    #[test]
    fn unavailable_entity_snapshots_to_unavailable_with_no_attrs() {
        let l = Light { on: true, brightness: 200, reachable: false };
        let (state, attrs) = l.state_snapshot();
        assert_eq!(state, STATE_UNAVAILABLE);
        assert!(attrs.is_empty());
    }

    #[test]
    fn none_state_becomes_unknown() {
        struct Sensor;
        impl Entity for Sensor {
            fn state(&self) -> Option<String> {
                None
            }
            fn name(&self) -> Option<String> {
                Some("Temp".into())
            }
            fn device_class(&self) -> Option<String> {
                Some("temperature".into())
            }
        }
        let (state, attrs) = Sensor.state_snapshot();
        assert_eq!(state, STATE_UNKNOWN);
        assert_eq!(attrs[ATTR_DEVICE_CLASS], json!("temperature"));
        assert_eq!(attrs[ATTR_FRIENDLY_NAME], json!("Temp"));
        // supported_features 0 → omitted
        assert!(!attrs.contains_key(ATTR_SUPPORTED_FEATURES));
    }

    #[test]
    fn assumed_state_and_attribution_folded_when_present() {
        struct Switch;
        impl Entity for Switch {
            fn state(&self) -> Option<String> {
                Some("on".into())
            }
            fn assumed_state(&self) -> bool {
                true
            }
            fn attribution(&self) -> Option<String> {
                Some("Data by ACME".into())
            }
            fn icon(&self) -> Option<String> {
                Some("mdi:toggle-switch".into())
            }
        }
        let (_state, attrs) = Switch.state_snapshot();
        assert_eq!(attrs[ATTR_ASSUMED_STATE], json!(true));
        assert_eq!(attrs[ATTR_ATTRIBUTION], json!("Data by ACME"));
        assert_eq!(attrs[ATTR_ICON], json!("mdi:toggle-switch"));
    }

    #[test]
    fn device_info_serde_round_trip() {
        let di = DeviceInfo {
            identifiers: vec![("hue".into(), "00:17".into())],
            manufacturer: Some("Signify".into()),
            model: Some("LCT015".into()),
            via_device: Some(("hue".into(), "bridge".into())),
            ..DeviceInfo::default()
        };
        let s = serde_json::to_string(&di).expect("ser");
        let back: DeviceInfo = serde_json::from_str(&s).expect("de");
        assert_eq!(back, di);
        // omitted optionals stay absent on the wire
        assert!(!s.contains("sw_version"));
    }
}
