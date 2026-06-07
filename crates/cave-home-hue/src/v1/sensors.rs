// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v1/sensors.py
//! v1 sensors controller. Ports `aiohue.v1.sensors` line-by-line.
//!
//! Hue sensors come in many flavours: physical Zigbee dimmer switches (ZLL),
//! Tap switches (ZGP), motion + light-level + temperature combos, plus a
//! family of "CLIP" virtual sensors backed by REST `PUT`s.

use crate::errors::HueResult;
use crate::v1::api::{ApiItems, RawItem, V1Item, V1Request};
use serde_json::{Value, json};

// ----- type constants ------------------------------------------------------

pub const TYPE_DAYLIGHT: &str = "Daylight";

pub const TYPE_CLIP_GENERICFLAG: &str = "CLIPGenericFlag";
pub const TYPE_CLIP_GENERICSTATUS: &str = "CLIPGenericStatus";
pub const TYPE_CLIP_HUMIDITY: &str = "CLIPHumidity";
pub const TYPE_CLIP_LIGHTLEVEL: &str = "CLIPLightLevel";
pub const TYPE_CLIP_OPENCLOSE: &str = "CLIPOpenClose";
pub const TYPE_CLIP_PRESENCE: &str = "CLIPPresence";
pub const TYPE_CLIP_SWITCH: &str = "CLIPSwitch";
pub const TYPE_CLIP_TEMPERATURE: &str = "CLIPTemperature";

pub const TYPE_GEOFENCE: &str = "Geofence";
pub const TYPE_ZGP_SWITCH: &str = "ZGPSwitch";

pub const TYPE_ZLL_LIGHTLEVEL: &str = "ZLLLightLevel";
pub const TYPE_ZLL_PRESENCE: &str = "ZLLPresence";
pub const TYPE_ZLL_ROTARY: &str = "ZLLRelativeRotary";
pub const TYPE_ZLL_SWITCH: &str = "ZLLSwitch";
pub const TYPE_ZLL_TEMPERATURE: &str = "ZLLTemperature";

// ZGP button event codes (Tap switch).
pub const ZGP_SWITCH_BUTTON_1: u16 = 34;
pub const ZGP_SWITCH_BUTTON_2: u16 = 16;
pub const ZGP_SWITCH_BUTTON_3: u16 = 17;
pub const ZGP_SWITCH_BUTTON_4: u16 = 18;

// ZLL button event codes (Dimmer switch, Smart Button etc).
pub const ZLL_SWITCH_BUTTON_1_INITIAL_PRESS: u16 = 1000;
pub const ZLL_SWITCH_BUTTON_2_INITIAL_PRESS: u16 = 2000;
pub const ZLL_SWITCH_BUTTON_3_INITIAL_PRESS: u16 = 3000;
pub const ZLL_SWITCH_BUTTON_4_INITIAL_PRESS: u16 = 4000;

pub const ZLL_SWITCH_BUTTON_1_HOLD: u16 = 1001;
pub const ZLL_SWITCH_BUTTON_2_HOLD: u16 = 2001;
pub const ZLL_SWITCH_BUTTON_3_HOLD: u16 = 3001;
pub const ZLL_SWITCH_BUTTON_4_HOLD: u16 = 4001;

pub const ZLL_SWITCH_BUTTON_1_SHORT_RELEASED: u16 = 1002;
pub const ZLL_SWITCH_BUTTON_2_SHORT_RELEASED: u16 = 2002;
pub const ZLL_SWITCH_BUTTON_3_SHORT_RELEASED: u16 = 3002;
pub const ZLL_SWITCH_BUTTON_4_SHORT_RELEASED: u16 = 4002;

pub const ZLL_SWITCH_BUTTON_1_LONG_RELEASED: u16 = 1003;
pub const ZLL_SWITCH_BUTTON_2_LONG_RELEASED: u16 = 2003;
pub const ZLL_SWITCH_BUTTON_3_LONG_RELEASED: u16 = 3003;
pub const ZLL_SWITCH_BUTTON_4_LONG_RELEASED: u16 = 4003;

pub const EVENT_BUTTON: &str = "button";
pub const EVENT_LIGHTLEVEL: &str = "light_level";
pub const EVENT_MOTION: &str = "motion";
pub const EVENT_POWER: &str = "device_power";
pub const EVENT_TEMPERATURE: &str = "temperature";

// ----- generic sensor ------------------------------------------------------

/// Shared sensor base. Source: `aiohue.v1.sensors.GenericSensor`.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericSensor {
    pub id: String,
    pub raw: RawItem,
    /// Last event payload (e.g. `{"event": "button", "id": "<id>", ...}`).
    /// Source: `GenericSensor.last_event`. Updated by the bridge poller /
    /// EventStream router, not by the controller itself.
    pub last_event: Option<Value>,
}

impl V1Item for GenericSensor {
    const ITEM_TYPE: &'static str = "sensors";
    fn from_raw(id: String, raw: RawItem) -> Self {
        Self {
            id,
            raw,
            last_event: None,
        }
    }
    fn set_raw(&mut self, raw: RawItem) {
        self.raw = raw;
    }
}

impl GenericSensor {
    /// `aiohue.v1.sensors.GenericSensor.name`.
    #[must_use]
    pub fn name(&self) -> &str {
        self.raw.get("name").and_then(Value::as_str).unwrap_or("")
    }
    /// `aiohue.v1.sensors.GenericSensor.type`.
    #[must_use]
    pub fn sensor_type(&self) -> &str {
        self.raw.get("type").and_then(Value::as_str).unwrap_or("")
    }
    /// `aiohue.v1.sensors.GenericSensor.modelid`.
    #[must_use]
    pub fn model_id(&self) -> Option<&str> {
        self.raw.get("modelid").and_then(Value::as_str)
    }
    /// `aiohue.v1.sensors.GenericSensor.manufacturername`.
    #[must_use]
    pub fn manufacturer_name(&self) -> Option<&str> {
        self.raw.get("manufacturername").and_then(Value::as_str)
    }
    /// `aiohue.v1.sensors.GenericSensor.productname` (1.24+).
    #[must_use]
    pub fn product_name(&self) -> Option<&str> {
        self.raw.get("productname").and_then(Value::as_str)
    }
    /// `aiohue.v1.sensors.GenericSensor.uniqueid`.
    #[must_use]
    pub fn unique_id(&self) -> Option<&str> {
        self.raw.get("uniqueid").and_then(Value::as_str)
    }
    /// `aiohue.v1.sensors.GenericSensor.swversion`.
    #[must_use]
    pub fn sw_version(&self) -> Option<&str> {
        self.raw.get("swversion").and_then(Value::as_str)
    }
    /// `aiohue.v1.sensors.GenericSensor.state` — sensor "state" subtree.
    #[must_use]
    pub fn state(&self) -> Option<&serde_json::Map<String, Value>> {
        self.raw.get("state").and_then(Value::as_object)
    }
    /// `aiohue.v1.sensors.GenericSensor.config` — sensor "config" subtree.
    #[must_use]
    pub fn config(&self) -> Option<&serde_json::Map<String, Value>> {
        self.raw.get("config").and_then(Value::as_object)
    }
    /// Convenience: ZLL battery is in `state` but falls back to `config`.
    /// Source: `aiohue.v1.sensors.GenericZLLSensor.battery`.
    #[must_use]
    pub fn zll_battery(&self) -> Option<i64> {
        self.state()
            .and_then(|s| s.get("battery"))
            .and_then(Value::as_i64)
            .or_else(|| {
                self.config()
                    .and_then(|c| c.get("battery"))
                    .and_then(Value::as_i64)
            })
    }
    /// CLIP sensors expose `state.battery` directly.
    /// Source: `aiohue.v1.sensors.GenericCLIPSensor.battery`.
    #[must_use]
    pub fn clip_battery(&self) -> Option<i64> {
        self.state()
            .and_then(|s| s.get("battery"))
            .and_then(Value::as_i64)
    }
    /// `aiohue.v1.sensors.GenericCLIPSensor.lastupdated` — required field.
    /// For ZLL variants: `GenericZLLSensor.lastupdated` — optional.
    #[must_use]
    pub fn last_updated(&self) -> Option<&str> {
        self.state()
            .and_then(|s| s.get("lastupdated"))
            .and_then(Value::as_str)
    }
    /// `aiohue.v1.sensors.GenericCLIPSensor.on` / `GenericZLLSensor.on`.
    #[must_use]
    pub fn on(&self) -> Option<bool> {
        self.config()
            .and_then(|c| c.get("on"))
            .and_then(Value::as_bool)
    }
    /// `aiohue.v1.sensors.GenericCLIPSensor.reachable` / `.GenericZLLSensor.reachable`.
    #[must_use]
    pub fn reachable(&self) -> Option<bool> {
        self.config()
            .and_then(|c| c.get("reachable"))
            .and_then(Value::as_bool)
    }
    /// `aiohue.v1.sensors.GenericSwitchSensor.buttonevent`.
    #[must_use]
    pub fn button_event(&self) -> Option<i64> {
        self.state()
            .and_then(|s| s.get("buttonevent"))
            .and_then(Value::as_i64)
    }
    /// `aiohue.v1.sensors.DaylightSensor.daylight`.
    #[must_use]
    pub fn daylight(&self) -> Option<bool> {
        self.state()
            .and_then(|s| s.get("daylight"))
            .and_then(Value::as_bool)
    }
    /// `aiohue.v1.sensors.PresenceSensor.presence`.
    #[must_use]
    pub fn presence(&self) -> Option<bool> {
        self.state()
            .and_then(|s| s.get("presence"))
            .and_then(Value::as_bool)
    }
    /// `aiohue.v1.sensors.LightLevelSensor.lightlevel`.
    #[must_use]
    pub fn light_level(&self) -> Option<i64> {
        self.state()
            .and_then(|s| s.get("lightlevel"))
            .and_then(Value::as_i64)
    }
    /// `aiohue.v1.sensors.TemperatureSensor.temperature` — int * 100.
    #[must_use]
    pub fn temperature_centi(&self) -> Option<i64> {
        self.state()
            .and_then(|s| s.get("temperature"))
            .and_then(Value::as_i64)
    }

    /// `aiohue.v1.sensors.GenericCLIPSensor.set_state`.
    pub async fn set_state(
        &self,
        req: &dyn V1Request,
        state: Value,
    ) -> HueResult<()> {
        let _ = req
            .put(&format!("sensors/{}/state", self.id), state)
            .await?;
        Ok(())
    }

    /// `aiohue.v1.sensors.GenericCLIPSensor.set_config` /
    /// `GenericSwitchSensor.set_config` — switch wraps `{on: bool}`, CLIP
    /// passes the raw dict. We expose the raw form.
    pub async fn set_config(
        &self,
        req: &dyn V1Request,
        config: Value,
    ) -> HueResult<()> {
        let _ = req
            .put(&format!("sensors/{}/config", self.id), config)
            .await?;
        Ok(())
    }

    /// Convenience over `set_config` for switch-style sensors, where the
    /// only writable knob is `{on: bool}`. Source:
    /// `aiohue.v1.sensors.GenericSwitchSensor.set_config`.
    pub async fn set_switch_on(
        &self,
        req: &dyn V1Request,
        on: Option<bool>,
    ) -> HueResult<()> {
        let body = match on {
            Some(value) => json!({ "on": value }),
            None => json!({}),
        };
        self.set_config(req, body).await
    }
}

/// `aiohue.v1.sensors.Sensors` — map of sensors keyed by ID.
pub type Sensors = ApiItems<GenericSensor>;

#[must_use]
pub fn new_sensors() -> Sensors {
    ApiItems::new("sensors")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dimmer_switch_raw() -> RawItem {
        json!({
            "name": "Salon Dimmer",
            "type": TYPE_ZLL_SWITCH,
            "modelid": "RWL022",
            "manufacturername": "Signify",
            "uniqueid": "00:17:88:00:00:00:00:01-02-fc00",
            "state": {"buttonevent": ZLL_SWITCH_BUTTON_1_INITIAL_PRESS as i64, "lastupdated": "2026-05-17T20:00:00"},
            "config": {"on": true, "reachable": true, "battery": 90},
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn zll_switch_exposes_button_event_and_battery() {
        let s = GenericSensor::from_raw("1".into(), dimmer_switch_raw());
        assert_eq!(s.sensor_type(), TYPE_ZLL_SWITCH);
        assert_eq!(s.button_event(), Some(1000));
        assert_eq!(s.zll_battery(), Some(90));
        assert_eq!(s.on(), Some(true));
        assert_eq!(s.reachable(), Some(true));
    }

    #[test]
    fn motion_sensor_state() {
        let raw = json!({
            "name": "Antre Motion",
            "type": TYPE_ZLL_PRESENCE,
            "state": {"presence": true, "lastupdated": "2026-05-17T20:01:00"},
            "config": {"on": true, "reachable": true, "battery": 80},
        });
        let s = GenericSensor::from_raw("4".into(), raw.as_object().unwrap().clone());
        assert_eq!(s.presence(), Some(true));
        assert_eq!(s.last_updated(), Some("2026-05-17T20:01:00"));
    }

    #[test]
    fn daylight_sensor_exposes_daylight() {
        let raw = json!({
            "name": "Daylight",
            "type": TYPE_DAYLIGHT,
            "state": {"daylight": false, "lastupdated": "none"},
            "config": {"on": true, "configured": true, "sunriseoffset": 30, "sunsetoffset": -30},
        });
        let s = GenericSensor::from_raw("1".into(), raw.as_object().unwrap().clone());
        assert_eq!(s.daylight(), Some(false));
    }

    #[test]
    fn temperature_value_is_centidegrees() {
        let raw = json!({
            "name": "T1",
            "type": TYPE_ZLL_TEMPERATURE,
            "state": {"temperature": 2150, "lastupdated": "2026-05-17T20:00:00"},
            "config": {"on": true, "reachable": true},
        });
        let s = GenericSensor::from_raw("5".into(), raw.as_object().unwrap().clone());
        // 2150 = 21.50°C — upstream returns the raw int; consumers divide.
        assert_eq!(s.temperature_centi(), Some(2150));
    }
}
