// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/light.py
//! v2 Light resource. Mirrors `aiohue.v2.models.light`.
//!
//! Reference: <https://developers.meethue.com/develop/hue-api-v2/api-reference/#resource_light>

use crate::v2::models::feature::{
    ColorFeature, ColorFeatureBase, ColorTemperatureFeature, ColorTemperatureFeatureBase,
    DimmingDeltaFeaturePut, DimmingFeature, DimmingFeatureBase, DynamicStatus, DynamicsFeature,
    OnFeature,
};
use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.light.LightMetaData`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightMetaData {
    /// Light archetype. Deprecated upstream — use the device-level
    /// archetype on the owning device. Preserved here for compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archetype: Option<String>,
    pub name: String,
}

/// `aiohue.v2.models.light.LightMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightMode {
    Normal,
    Streaming,
}

/// `aiohue.v2.models.light.Light`. Resource as returned by `clip/v2/resource/light/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub id: String,
    pub owner: ResourceIdentifier,
    pub on: OnFeature,
    pub mode: LightMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<LightMetaData>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimming: Option<DimmingFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temperature: Option<ColorTemperatureFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamics: Option<DynamicsFeature>,

    #[serde(default = "default_resource_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_resource_type() -> ResourceTypes {
    ResourceTypes::Light
}

impl Light {
    /// `aiohue.v2.models.light.Light.supports_dimming`.
    #[must_use]
    pub const fn supports_dimming(&self) -> bool {
        self.dimming.is_some()
    }
    /// `aiohue.v2.models.light.Light.supports_color`.
    #[must_use]
    pub const fn supports_color(&self) -> bool {
        self.color.is_some()
    }
    /// `aiohue.v2.models.light.Light.supports_color_temperature`.
    #[must_use]
    pub const fn supports_color_temperature(&self) -> bool {
        self.color_temperature.is_some()
    }
    /// `aiohue.v2.models.light.Light.is_on`.
    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.on.on
    }
    /// `aiohue.v2.models.light.Light.brightness`. Falls back to 100/0 like
    /// the upstream property when the light doesn't expose dimming.
    #[must_use]
    pub fn brightness(&self) -> f32 {
        match self.dimming {
            Some(d) => d.brightness,
            None if self.is_on() => 100.0,
            None => 0.0,
        }
    }
    /// `aiohue.v2.models.light.Light.is_dynamic`.
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        matches!(
            self.dynamics.as_ref().and_then(|d| d.status),
            Some(DynamicStatus::DynamicPalette)
        )
    }
    /// `aiohue.v2.models.light.Light.entertainment_active`.
    #[must_use]
    pub fn entertainment_active(&self) -> bool {
        matches!(self.mode, LightMode::Streaming)
    }
}

/// `aiohue.v2.models.light.LightPut`. PUT body for `/clip/v2/resource/light/{id}`.
#[derive(Debug, Default, Clone, Serialize)]
pub struct LightPut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<OnFeature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimming: Option<DimmingFeatureBase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimming_delta: Option<DimmingDeltaFeaturePut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_temperature: Option<ColorTemperatureFeatureBase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorFeatureBase>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::models::feature::{ColorPoint, OnFeature};
    use serde_json::json;

    fn sample_light() -> Light {
        serde_json::from_value(json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "owner": {"rid": "device-1", "rtype": "device"},
            "on": {"on": true},
            "mode": "normal",
            "metadata": {"name": "Mutfak"},
            "dimming": {"brightness": 60.0, "min_dim_level": 0.5},
            "color": {"xy": {"x": 0.4, "y": 0.4}, "gamut_type": "C"},
            "color_temperature": {"mirek": 366, "mirek_valid": true},
            "type": "light"
        }))
        .unwrap()
    }

    #[test]
    fn light_supports_features_flags() {
        let l = sample_light();
        assert!(l.supports_dimming());
        assert!(l.supports_color());
        assert!(l.supports_color_temperature());
        assert!(l.is_on());
        assert!((l.brightness() - 60.0).abs() < 1e-3);
        assert!(!l.entertainment_active());
        assert!(!l.is_dynamic());
    }

    #[test]
    fn brightness_falls_back_when_no_dimming() {
        let mut l = sample_light();
        l.dimming = None;
        assert!((l.brightness() - 100.0).abs() < 1e-3);
        l.on.on = false;
        assert!((l.brightness() - 0.0).abs() < 1e-3);
    }

    #[test]
    fn light_put_serialises_only_present_fields() {
        let put = LightPut {
            on: Some(OnFeature { on: true }),
            dimming: Some(DimmingFeatureBase { brightness: 50.0 }),
            color: Some(ColorFeatureBase {
                xy: ColorPoint { x: 0.3, y: 0.3 },
            }),
            ..Default::default()
        };
        let s = serde_json::to_string(&put).unwrap();
        assert!(s.contains("\"on\":{\"on\":true}"));
        assert!(s.contains("\"dimming\":{\"brightness\":50.0}"));
        assert!(!s.contains("color_temperature_delta"));
    }

    #[test]
    fn light_id_v1_optional() {
        let l = sample_light();
        assert!(l.id_v1.is_none());
    }
}
