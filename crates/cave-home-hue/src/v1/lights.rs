// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v1/lights.py
//! v1 lights controller. Ports `aiohue.v1.lights` line-by-line.
//!
//! Reference: <https://developers.meethue.com/documentation/lights-api>.

use crate::errors::HueResult;
use crate::num::f64_to_f32;
use crate::v1::api::{ApiItems, RawItem, V1Item, V1Request};
use serde::Serialize;
use serde_json::{Value, json};

/// A CIE 1931 (x, y) chromaticity pair. Source: `aiohue.v1.lights.XYPoint`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct XYPoint {
    pub x: f32,
    pub y: f32,
}

/// Triangle gamut delimited by three primary [`XYPoint`]s. Source:
/// `aiohue.v1.lights.GamutType`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GamutType {
    pub red: XYPoint,
    pub green: XYPoint,
    pub blue: XYPoint,
}

/// State change to apply to a light. Source: parameter set of
/// `aiohue.v1.lights.Light.set_state`.
///
/// The Python version takes 14 keyword args, all optional. We pack them in
/// a struct with `Option<T>` fields and rely on `Default` for "send only
/// what was set", matching the upstream `{key: value for ... if value is
/// not None}` filter.
#[derive(Debug, Default, Clone, Serialize)]
pub struct LightStateUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bri: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hue: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sat: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy: Option<(f32, f32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ct: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(rename = "transitiontime", skip_serializing_if = "Option::is_none")]
    pub transition_time: Option<u16>,
    #[serde(rename = "bri_inc", skip_serializing_if = "Option::is_none")]
    pub bri_inc: Option<i16>,
    #[serde(rename = "sat_inc", skip_serializing_if = "Option::is_none")]
    pub sat_inc: Option<i16>,
    #[serde(rename = "hue_inc", skip_serializing_if = "Option::is_none")]
    pub hue_inc: Option<i32>,
    #[serde(rename = "ct_inc", skip_serializing_if = "Option::is_none")]
    pub ct_inc: Option<i16>,
    #[serde(rename = "xy_inc", skip_serializing_if = "Option::is_none")]
    pub xy_inc: Option<(f32, f32)>,
}

/// Single v1 light. Source: `aiohue.v1.lights.Light`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Light {
    pub id: String,
    pub raw: RawItem,
}

impl V1Item for Light {
    const ITEM_TYPE: &'static str = "lights";
    fn from_raw(id: String, raw: RawItem) -> Self {
        Self { id, raw }
    }
    fn set_raw(&mut self, raw: RawItem) {
        self.raw = raw;
    }
}

impl Light {
    /// `aiohue.v1.lights.Light.uniqueid` — Zigbee unique-id.
    #[must_use]
    pub fn unique_id(&self) -> Option<&str> {
        self.raw.get("uniqueid").and_then(Value::as_str)
    }
    /// `aiohue.v1.lights.Light.name`.
    #[must_use]
    pub fn name(&self) -> &str {
        self.raw.get("name").and_then(Value::as_str).unwrap_or("")
    }
    /// `aiohue.v1.lights.Light.manufacturername`.
    #[must_use]
    pub fn manufacturer_name(&self) -> Option<&str> {
        self.raw.get("manufacturername").and_then(Value::as_str)
    }
    /// `aiohue.v1.lights.Light.modelid`.
    #[must_use]
    pub fn model_id(&self) -> Option<&str> {
        self.raw.get("modelid").and_then(Value::as_str)
    }
    /// `aiohue.v1.lights.Light.productname` — added in 1.24 (2018-03-05).
    #[must_use]
    pub fn product_name(&self) -> Option<&str> {
        self.raw.get("productname").and_then(Value::as_str)
    }
    /// `aiohue.v1.lights.Light.swversion`.
    #[must_use]
    pub fn sw_version(&self) -> Option<&str> {
        self.raw.get("swversion").and_then(Value::as_str)
    }
    /// `aiohue.v1.lights.Light.type`.
    #[must_use]
    pub fn light_type(&self) -> Option<&str> {
        self.raw.get("type").and_then(Value::as_str)
    }
    /// `aiohue.v1.lights.Light.state` — the live state block.
    #[must_use]
    pub fn state(&self) -> Option<&serde_json::Map<String, Value>> {
        self.raw.get("state").and_then(Value::as_object)
    }
    /// `aiohue.v1.lights.Light.controlcapabilities` — capabilities.control.
    #[must_use]
    pub fn control_capabilities(&self) -> Option<&serde_json::Map<String, Value>> {
        self.raw
            .get("capabilities")
            .and_then(Value::as_object)
            .and_then(|c| c.get("control"))
            .and_then(Value::as_object)
    }
    /// `aiohue.v1.lights.Light.colorgamuttype` — "A" / "B" / "C" / "None".
    #[must_use]
    pub fn color_gamut_type(&self) -> &str {
        self.control_capabilities()
            .and_then(|c| c.get("colorgamuttype"))
            .and_then(Value::as_str)
            .unwrap_or("None")
    }
    /// `aiohue.v1.lights.Light.colorgamut`.
    #[must_use]
    pub fn color_gamut(&self) -> Option<GamutType> {
        let arr = self.control_capabilities()?.get("colorgamut")?.as_array()?;
        if arr.len() != 3 {
            return None;
        }
        let mut points = [None, None, None];
        for (idx, item) in arr.iter().enumerate() {
            let pair = item.as_array()?;
            if pair.len() != 2 {
                return None;
            }
            points[idx] = Some(XYPoint {
                x: f64_to_f32(pair[0].as_f64()?),
                y: f64_to_f32(pair[1].as_f64()?),
            });
        }
        Some(GamutType {
            red: points[0]?,
            green: points[1]?,
            blue: points[2]?,
        })
    }

    /// `aiohue.v1.lights.Light.set_state` — change live state.
    pub async fn set_state(&self, req: &dyn V1Request, update: &LightStateUpdate) -> HueResult<()> {
        let body = serde_json::to_value(update).unwrap_or_else(|_| json!({}));
        let _ = req.put(&format!("lights/{}/state", self.id), body).await?;
        Ok(())
    }
}

/// Map of v1 lights. Source: `aiohue.v1.lights.Lights`.
pub type Lights = ApiItems<Light>;

/// Build an empty `Lights` collection, mirroring `Lights.__init__`.
#[must_use]
pub fn new_lights() -> Lights {
    ApiItems::new("lights")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn light_exposes_upstream_fields() -> TestResult {
        let raw = json!({
            "name": "Mutfak Lambası",
            "uniqueid": "00:17:88:01:00:00:00:01-0b",
            "manufacturername": "Signify",
            "modelid": "LCT012",
            "productname": "Hue color candle",
            "swversion": "1.50.2",
            "type": "Extended color light",
            "state": {"on": true, "bri": 200},
            "capabilities": {
                "control": {
                    "colorgamuttype": "C",
                    "colorgamut": [[0.6915, 0.3083], [0.17, 0.7], [0.1532, 0.0475]],
                }
            }
        });
        let light = Light::from_raw("1".into(), raw_map(raw)?);
        assert_eq!(light.name(), "Mutfak Lambası");
        assert_eq!(light.unique_id(), Some("00:17:88:01:00:00:00:01-0b"));
        assert_eq!(light.manufacturer_name(), Some("Signify"));
        assert_eq!(light.model_id(), Some("LCT012"));
        assert_eq!(light.product_name(), Some("Hue color candle"));
        assert_eq!(light.sw_version(), Some("1.50.2"));
        assert_eq!(light.color_gamut_type(), "C");
        let gamut = light
            .color_gamut()
            .ok_or("`color_gamut()` is None for a complete gamut")?;
        assert!((gamut.red.x - 0.6915).abs() < 1e-3);
        Ok(())
    }

    #[test]
    fn light_state_update_serialises_only_set_fields() -> TestResult {
        let upd = LightStateUpdate {
            on: Some(true),
            bri: Some(254),
            ..LightStateUpdate::default()
        };
        let body = serde_json::to_value(&upd)?;
        let obj = body
            .as_object()
            .ok_or("the serialised update is not a JSON object")?;
        assert_eq!(obj.len(), 2); // only `on` and `bri`
        assert_eq!(
            obj.get("on").ok_or("the body has no `on`")?,
            &Value::Bool(true)
        );
        Ok(())
    }

    #[test]
    fn light_state_update_renames_transition_time_to_transitiontime() -> TestResult {
        let upd = LightStateUpdate {
            transition_time: Some(4),
            ..Default::default()
        };
        let body = serde_json::to_value(&upd)?;
        assert!(body.get("transitiontime").is_some());
        assert!(body.get("transition_time").is_none());
        Ok(())
    }

    #[test]
    fn missing_gamut_returns_none() -> TestResult {
        let raw = json!({"name": "x", "capabilities": {"control": {}}});
        let light = Light::from_raw("9".into(), raw_map(raw)?);
        assert!(light.color_gamut().is_none());
        assert_eq!(light.color_gamut_type(), "None");
        Ok(())
    }

    use crate::v1::api::test_support::{RecordingReq, raw_map};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// A light whose gamut has `red_x` as the red x coordinate.
    fn light_with_red_x(red_x: &Value) -> Result<Light, &'static str> {
        let raw = json!({
            "capabilities": {
                "control": {
                    "colorgamut": [[red_x, 0.3083], [0.17, 0.7], [0.1532, 0.0475]],
                }
            }
        });
        Ok(Light::from_raw("1".into(), raw_map(raw)?))
    }

    /// `f32` bit pattern `color_gamut()` reports for a red x coordinate.
    fn red_x_bits(red_x: &Value) -> Result<Option<u32>, &'static str> {
        Ok(light_with_red_x(red_x)?
            .color_gamut()
            .map(|gamut| gamut.red.x.to_bits()))
    }

    #[test]
    fn color_gamut_narrows_every_coordinate_to_the_nearest_f32() -> TestResult {
        let gamut = light_with_red_x(&json!(0.6915))?
            .color_gamut()
            .ok_or("`color_gamut()` is None for a complete gamut")?;
        assert_eq!(gamut.red.x.to_bits(), 0x3f31_0625); // 0.6915
        assert_eq!(gamut.red.y.to_bits(), 0x3e9d_d97f); // 0.3083
        assert_eq!(gamut.green.x.to_bits(), 0x3e2e_147b); // 0.17
        assert_eq!(gamut.green.y.to_bits(), 0x3f33_3333); // 0.7
        assert_eq!(gamut.blue.x.to_bits(), 0x3e1c_e076); // 0.1532
        assert_eq!(gamut.blue.y.to_bits(), 0x3d42_8f5c); // 0.0475
        Ok(())
    }

    #[test]
    fn color_gamut_coordinate_edge_values_keep_their_f32_bits() -> TestResult {
        let cases = [
            (json!(0), 0x0000_0000),
            (json!(0.0), 0x0000_0000),
            (json!(-0.0), 0x8000_0000),
            (json!(1), 0x3f80_0000),
            (json!(1.0), 0x3f80_0000),
            (json!(0.5), 0x3f00_0000),
            (json!(0.1), 0x3dcc_cccd),
            (json!(0.300_000_000_000_000_04), 0x3e99_999a),
            (json!(-0.25), 0xbe80_0000),
            (json!(2.5), 0x4020_0000),
            (json!(253), 0x437d_0000),
            (json!(254), 0x437e_0000),
            (json!(255), 0x437f_0000),
            (json!(65_535), 0x477f_ff00),
            (json!(16_777_217), 0x4b80_0000), // 2^24 + 1: tie, rounds to even
            (json!(u64::MAX), 0x5f80_0000),   // 2^64 after the f64 step
            (json!(i64::MIN), 0xdf00_0000),   // -2^63
            (json!(1e-50), 0x0000_0000),      // underflows to zero
            (json!(-1e-50), 0x8000_0000),     // ... keeping the sign
            (json!(1e40), 0x7f80_0000),       // overflows to +infinity
            (json!(-1e40), 0xff80_0000),      // overflows to -infinity
            (json!(f64::MAX), 0x7f80_0000),
        ];
        for (red_x, expected) in &cases {
            assert_eq!(red_x_bits(red_x)?, Some(*expected), "red x = {red_x}");
        }
        Ok(())
    }

    #[test]
    fn color_gamut_is_none_when_a_coordinate_is_not_a_number() -> TestResult {
        // JSON has no NaN: `json!(f64::NAN)` is `null`, which is not a number.
        for red_x in [json!(f64::NAN), json!(null), json!("0.5"), json!([0.5])] {
            assert_eq!(red_x_bits(&red_x)?, None, "red x = {red_x}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn set_state_puts_exactly_the_set_fields() -> TestResult {
        let req = RecordingReq::default();
        let light = Light::from_raw("7".into(), serde_json::Map::new());
        let update = LightStateUpdate {
            on: Some(true),
            bri: Some(254),
            hue: Some(65_535),
            sat: Some(0),
            xy: Some((0.5, 0.25)),
            ct: Some(153),
            transition_time: Some(4),
            bri_inc: Some(-254),
            ..LightStateUpdate::default()
        };
        light.set_state(&req, &update).await?;
        light.set_state(&req, &LightStateUpdate::default()).await?;
        let puts = req.puts.lock().clone();
        assert_eq!(
            puts,
            vec![
                (
                    "lights/7/state".to_string(),
                    json!({
                        "on": true,
                        "bri": 254,
                        "hue": 65_535,
                        "sat": 0,
                        "xy": [0.5, 0.25],
                        "ct": 153,
                        "transitiontime": 4,
                        "bri_inc": -254,
                    }),
                ),
                ("lights/7/state".to_string(), json!({})),
            ]
        );
        Ok(())
    }
}
