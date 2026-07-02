// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/models/feature.py
//! v2 feature sub-objects — `dimming`, `color`, `color_temperature`, `on`,
//! `dynamics`, `alert`, `effects`, ...
//!
//! Upstream `feature.py` is ~700 lines of dataclasses; we port the headline
//! features used by [`super::light::Light`] + [`super::scene::Scene`]. The
//! full list is tracked in `parity.manifest.toml`.

use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.feature.OnFeature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnFeature {
    pub on: bool,
}

/// `aiohue.v2.models.feature.DimmingFeature` (state). Brightness is
/// percent (0.0..=100.0), upstream uses `float`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DimmingFeature {
    pub brightness: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_dim_level: Option<f32>,
}

/// `aiohue.v2.models.feature.DimmingFeatureBase` — PUT shape (no minimum).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DimmingFeatureBase {
    pub brightness: f32,
}

/// `aiohue.v2.models.feature.DimmingDeltaFeaturePut` — relative dimming.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DimmingDeltaFeaturePut {
    pub action: DimmingDeltaAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness_delta: Option<f32>,
}

/// Source: `aiohue.v2.models.feature.DimmingDeltaAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimmingDeltaAction {
    Up,
    Down,
    Stop,
}

/// `aiohue.v2.models.feature.ColorTemperatureFeatureBase` — PUT shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorTemperatureFeatureBase {
    pub mirek: Option<u16>,
}

/// `aiohue.v2.models.feature.ColorTemperatureFeature` — full state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorTemperatureFeature {
    #[serde(default)]
    pub mirek: Option<u16>,
    #[serde(default)]
    pub mirek_valid: Option<bool>,
    #[serde(default)]
    pub mirek_schema: Option<MirekSchema>,
}

/// `aiohue.v2.models.feature.MirekSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirekSchema {
    pub mirek_minimum: u16,
    pub mirek_maximum: u16,
}

/// `aiohue.v2.models.feature.ColorPoint` — CIE xy coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorPoint {
    pub x: f32,
    pub y: f32,
}

/// `aiohue.v2.models.feature.ColorFeatureBase` — PUT shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorFeatureBase {
    pub xy: ColorPoint,
}

/// `aiohue.v2.models.feature.ColorFeature` — full state including gamut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorFeature {
    pub xy: ColorPoint,
    #[serde(default)]
    pub gamut: Option<Gamut>,
    #[serde(default)]
    pub gamut_type: Option<String>,
}

/// `aiohue.v2.models.feature.Gamut`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gamut {
    pub red: ColorPoint,
    pub green: ColorPoint,
    pub blue: ColorPoint,
}

/// `aiohue.v2.models.feature.DynamicStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicStatus {
    None,
    DynamicPalette,
}

/// `aiohue.v2.models.feature.DynamicsFeature`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsFeature {
    #[serde(default)]
    pub status: Option<DynamicStatus>,
    #[serde(default)]
    pub status_values: Option<Vec<String>>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub speed_valid: Option<bool>,
}

/// `aiohue.v2.models.feature.ButtonReportEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonReportEvent {
    InitialPress,
    Repeat,
    ShortRelease,
    LongRelease,
    LongPress,
    DoubleShortRelease,
}

/// `aiohue.v2.models.feature.ButtonReport`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonReport {
    pub updated: String,
    pub event: ButtonReportEvent,
}

/// `aiohue.v2.models.feature.MotionReport`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionReport {
    pub changed: String,
    pub motion: bool,
}

/// `aiohue.v2.models.feature.AlertEffectType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertEffectType {
    Breathe,
    NoAlert,
}

/// `aiohue.v2.models.feature.AlertFeature`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertFeature {
    pub action: AlertEffectType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimming_feature_round_trips() {
        let v = DimmingFeature {
            brightness: 75.0,
            min_dim_level: Some(1.0),
        };
        let s = serde_json::to_string(&v).unwrap();
        let back: DimmingFeature = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn color_feature_decodes_minimal_xy_only() {
        let s = r#"{"xy":{"x":0.4,"y":0.5}}"#;
        let v: ColorFeature = serde_json::from_str(s).unwrap();
        assert!(v.gamut.is_none());
        assert!(v.gamut_type.is_none());
        assert!((v.xy.x - 0.4).abs() < 1e-6);
    }

    #[test]
    fn button_report_event_round_trip() {
        let json = r#"{"updated":"2026-05-17T20:00:00Z","event":"short_release"}"#;
        let r: ButtonReport = serde_json::from_str(json).unwrap();
        assert_eq!(r.event, ButtonReportEvent::ShortRelease);
    }

    #[test]
    fn dimming_delta_action_serialises_snake_case() {
        let v = DimmingDeltaFeaturePut {
            action: DimmingDeltaAction::Up,
            brightness_delta: Some(10.0),
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"action\":\"up\""));
    }
}
