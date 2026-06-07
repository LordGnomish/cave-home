// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Network-application wire DTOs and their mapping onto the
//! [`cave_home_unifi_network`] domain model.
//!
//! Every Network-controller response is the envelope `{ "meta": {"rc", "msg"},
//! "data": [...] }` — `rc` is `"ok"` or `"error"` **even on HTTP 200**, so
//! [`Envelope::into_data`] is where that is enforced. The `Wire*` structs mirror
//! the documented `stat/sta`, `stat/device`, `stat/event`, `self/sites` and
//! `stat/health` shapes (only the fields cave-home reasons about), and each
//! lowers to the transport-free domain type the sibling crate already tests.

use serde::Deserialize;

use cave_home_unifi_network::{
    DeviceKind, NetworkClient, NetworkDevice, SwitchPort,
};

use crate::error::UnifiError;

/// The `meta` block every Network response carries.
#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    /// Result code: `"ok"` or `"error"`.
    #[serde(default)]
    pub rc: String,
    /// Optional human/diagnostic message (an `api.err.*` key on failure).
    #[serde(default)]
    pub msg: Option<String>,
}

/// The `{ meta, data }` envelope around every Network response.
#[derive(Debug, Clone, Deserialize)]
pub struct Envelope<T> {
    /// The result metadata.
    pub meta: Meta,
    /// The payload list (empty for command acks).
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
}

impl<T> Envelope<T> {
    /// Unwrap the data, turning a `rc == "error"` envelope into a
    /// [`UnifiError::Http`] carrying the `meta.msg`.
    ///
    /// # Errors
    /// [`UnifiError::Http`] (status 0 — application-level) when `rc != "ok"`.
    pub fn into_data(self) -> crate::Result<Vec<T>> {
        if self.meta.rc == "ok" {
            Ok(self.data)
        } else {
            Err(UnifiError::Http {
                status: 0,
                message: self
                    .meta
                    .msg
                    .unwrap_or_else(|| "controller returned rc=error".into()),
                body: String::new(),
            })
        }
    }
}

/// A UniFi site (`self/sites`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Site {
    /// The site's internal short name (used in every `/api/s/{name}/...` path).
    pub name: String,
    /// The human description ("Default").
    #[serde(default, rename = "desc")]
    pub description: String,
    /// The signed-in user's role on this site, if reported.
    #[serde(default)]
    pub role: String,
}

/// A network client (`stat/sta`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireClient {
    /// MAC address (always present).
    #[serde(default)]
    pub mac: String,
    /// User-set alias, if any.
    #[serde(default)]
    pub name: Option<String>,
    /// DHCP / mDNS hostname, if any.
    #[serde(default)]
    pub hostname: Option<String>,
    /// Current IP, if leased.
    #[serde(default)]
    pub ip: Option<String>,
    /// Whether the client is on a wired port.
    #[serde(default)]
    pub is_wired: Option<bool>,
    /// SSID, when wireless.
    #[serde(default)]
    pub essid: Option<String>,
    /// Serving AP MAC, when wireless.
    #[serde(default)]
    pub ap_mac: Option<String>,
    /// Upstream switch MAC, when wired.
    #[serde(default)]
    pub sw_mac: Option<String>,
    /// Whether this client is on the guest network.
    #[serde(default)]
    pub is_guest: Option<bool>,
    /// Whether this client is currently blocked.
    #[serde(default)]
    pub blocked: Option<bool>,
    /// Last-seen unix time (seconds).
    #[serde(default)]
    pub last_seen: Option<u64>,
}

impl WireClient {
    /// Lower to the domain [`NetworkClient`].
    #[must_use]
    pub fn into_domain(self) -> NetworkClient {
        let name = self
            .name
            .filter(|s| !s.is_empty())
            .or_else(|| self.hostname.filter(|s| !s.is_empty()))
            .unwrap_or_else(|| self.mac.clone());
        let mut c = NetworkClient::new(self.mac, name);
        if self.is_wired.unwrap_or(false) {
            if let Some(sw) = self.sw_mac.filter(|s| !s.is_empty()) {
                c = c.wired_to(sw);
            }
        } else {
            let ssid = self.essid.unwrap_or_default();
            let ap = self.ap_mac.unwrap_or_default();
            c = c.wireless(ssid, ap);
        }
        if let Some(ip) = self.ip.and_then(|s| s.parse().ok()) {
            c = c.with_ip(ip);
        }
        if self.is_guest.unwrap_or(false) {
            c = c.as_guest();
        }
        if self.blocked.unwrap_or(false) {
            c = c.blocked();
        }
        if let Some(ls) = self.last_seen {
            c = c.last_seen_at(ls);
        }
        c
    }
}

/// A switch port row (`stat/device` → `port_table`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WirePort {
    /// 1-based port index.
    #[serde(default)]
    pub port_idx: u16,
    /// Whether the port can deliver PoE at all.
    #[serde(default)]
    pub port_poe: Option<bool>,
    /// Whether PoE is currently enabled/forced on the port.
    #[serde(default)]
    pub poe_enable: Option<bool>,
    /// Whether the port is actually sourcing power right now.
    #[serde(default)]
    pub poe_good: Option<bool>,
}

/// A network device (`stat/device`): switch / AP / gateway.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireDevice {
    /// The controller's internal device id.
    #[serde(default, rename = "_id")]
    pub id: String,
    /// MAC address.
    #[serde(default)]
    pub mac: String,
    /// User-set name, if any.
    #[serde(default)]
    pub name: Option<String>,
    /// Hardware model code.
    #[serde(default)]
    pub model: Option<String>,
    /// Device class: `usw` switch, `uap` AP, `ugw`/`udm` gateway.
    #[serde(default, rename = "type")]
    pub dev_type: String,
    /// Adoption/online state: 1 == connected.
    #[serde(default)]
    pub state: i64,
    /// Switch port table, when a switch.
    #[serde(default)]
    pub port_table: Vec<WirePort>,
    /// Uplink block, when not the gateway.
    #[serde(default)]
    pub uplink: Option<WireUplink>,
}

/// The uplink sub-object on a device.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireUplink {
    /// The MAC of the device this one uplinks to.
    #[serde(default)]
    pub uplink_mac: Option<String>,
}

impl WireDevice {
    /// Map the `type` code to a domain [`DeviceKind`]; unknown codes default to
    /// a switch (the most port-rich kind, and the safest for the UI).
    #[must_use]
    pub fn kind(&self) -> DeviceKind {
        match self.dev_type.as_str() {
            "uap" => DeviceKind::AccessPoint,
            "ugw" | "udm" | "uxg" | "ucg" => DeviceKind::Gateway,
            _ => DeviceKind::Switch,
        }
    }

    /// Lower to the domain [`NetworkDevice`].
    #[must_use]
    pub fn into_domain(self) -> NetworkDevice {
        let kind = self.kind();
        let name = self
            .name
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| self.mac.clone());
        let mut d = NetworkDevice::new(self.id, name, self.mac, kind);
        if let Some(model) = self.model {
            d = d.with_model(model);
        }
        if self.state != 1 {
            d = d.offline();
        }
        if let Some(up) = self.uplink.and_then(|u| u.uplink_mac).filter(|s| !s.is_empty()) {
            d = d.uplinked_to(up);
        }
        if !self.port_table.is_empty() {
            let ports = self
                .port_table
                .into_iter()
                .map(|p| {
                    let mut port = SwitchPort::new(p.port_idx, p.port_poe.unwrap_or(false));
                    port.poe_active = p.poe_good.or(p.poe_enable).unwrap_or(false);
                    port
                })
                .collect();
            d = d.with_ports(ports);
        }
        d
    }
}

/// A network event (`stat/event`): a switch came up, a client roamed, etc.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WireEvent {
    /// The event key, e.g. `EVT_SW_Connected`.
    #[serde(default)]
    pub key: String,
    /// The pre-rendered human message, when present.
    #[serde(default)]
    pub msg: Option<String>,
    /// Event time in unix milliseconds.
    #[serde(default)]
    pub time: i64,
}

/// A cave-home-shaped network event (the household-facing projection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkEvent {
    /// The raw event key (`EVT_*`).
    pub key: String,
    /// The human message (falls back to the key if the controller omitted one).
    pub message: String,
    /// Event time in unix milliseconds.
    pub time_ms: i64,
}

impl From<WireEvent> for NetworkEvent {
    fn from(w: WireEvent) -> Self {
        let message = w.msg.filter(|s| !s.is_empty()).unwrap_or_else(|| w.key.clone());
        Self {
            key: w.key,
            message,
            time_ms: w.time,
        }
    }
}

/// A health subsystem row (`stat/health`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HealthSubsystem {
    /// The subsystem name: `wan`, `lan`, `wlan`, `www`, ...
    #[serde(default)]
    pub subsystem: String,
    /// Status string: `ok`, `warning`, `error`.
    #[serde(default)]
    pub status: String,
}

impl HealthSubsystem {
    /// Whether this subsystem reports healthy.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.status == "ok"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_unifi_network::{ConnectionKind, DeviceState};

    #[test]
    fn envelope_ok_yields_data() {
        let env: Envelope<Site> = serde_json::from_str(
            r#"{"meta":{"rc":"ok"},"data":[{"name":"default","desc":"Default"}]}"#,
        )
        .unwrap();
        let sites = env.into_data().unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].name, "default");
        assert_eq!(sites[0].description, "Default");
    }

    #[test]
    fn envelope_error_becomes_unifi_error_with_msg() {
        let env: Envelope<Site> = serde_json::from_str(
            r#"{"meta":{"rc":"error","msg":"api.err.NoSiteContext"},"data":[]}"#,
        )
        .unwrap();
        let err = env.into_data().unwrap_err();
        assert!(err.to_string().contains("api.err.NoSiteContext"));
    }

    #[test]
    fn wire_client_wireless_maps_to_domain() {
        let w: WireClient = serde_json::from_str(
            r#"{"mac":"aa:bb:cc:dd:ee:01","hostname":"kid-tablet","ip":"192.168.1.50",
                "is_wired":false,"essid":"Home","ap_mac":"ap-1","is_guest":false,
                "blocked":true,"last_seen":1717000000}"#,
        )
        .unwrap();
        let c = w.into_domain();
        assert_eq!(c.mac(), "aa:bb:cc:dd:ee:01");
        assert_eq!(c.name(), "kid-tablet");
        assert!(c.is_wireless());
        assert_eq!(c.connection().ssid(), Some("Home"));
        assert!(c.is_blocked());
        assert_eq!(c.ip().map(|i| i.to_string()), Some("192.168.1.50".to_string()));
        assert_eq!(c.last_seen(), 1_717_000_000);
    }

    #[test]
    fn wire_client_wired_uses_switch_uplink() {
        let w: WireClient = serde_json::from_str(
            r#"{"mac":"aa:bb:cc:00:00:01","is_wired":true,"sw_mac":"sw-1"}"#,
        )
        .unwrap();
        let c = w.into_domain();
        assert!(!c.is_wireless());
        assert_eq!(c.connection(), &ConnectionKind::Wired);
        assert_eq!(c.uplink_device(), Some("sw-1"));
        // no name/hostname -> falls back to MAC
        assert_eq!(c.name(), "aa:bb:cc:00:00:01");
    }

    #[test]
    fn wire_device_switch_with_ports() {
        let w: WireDevice = serde_json::from_str(
            r#"{"_id":"d1","mac":"sw:mac","name":"Salon switch","model":"USW-24",
                "type":"usw","state":1,
                "port_table":[
                    {"port_idx":1,"port_poe":true,"poe_good":true},
                    {"port_idx":2,"port_poe":false}
                ]}"#,
        )
        .unwrap();
        let d = w.into_domain();
        assert_eq!(d.kind(), DeviceKind::Switch);
        assert_eq!(d.state(), DeviceState::Online);
        assert_eq!(d.model(), "USW-24");
        let p1 = d.port(1).unwrap();
        assert!(p1.poe_capable);
        assert!(p1.poe_active);
        let p2 = d.port(2).unwrap();
        assert!(!p2.poe_capable);
    }

    #[test]
    fn wire_device_offline_gateway_and_ap_kinds() {
        let gw: WireDevice =
            serde_json::from_str(r#"{"_id":"g","mac":"m","type":"udm","state":0}"#).unwrap();
        assert_eq!(gw.kind(), DeviceKind::Gateway);
        assert_eq!(gw.into_domain().state(), DeviceState::Offline);

        let ap: WireDevice = serde_json::from_str(
            r#"{"_id":"a","mac":"m","type":"uap","state":1,"uplink":{"uplink_mac":"sw-1"}}"#,
        )
        .unwrap();
        assert_eq!(ap.kind(), DeviceKind::AccessPoint);
        assert_eq!(ap.into_domain().uplink(), Some("sw-1"));
    }

    #[test]
    fn wire_event_falls_back_to_key_when_no_msg() {
        let e: WireEvent =
            serde_json::from_str(r#"{"key":"EVT_SW_Connected","time":1717000000000}"#).unwrap();
        let ev = NetworkEvent::from(e);
        assert_eq!(ev.message, "EVT_SW_Connected");
        assert_eq!(ev.time_ms, 1_717_000_000_000);
    }

    #[test]
    fn health_subsystem_ok_flag() {
        let h: HealthSubsystem =
            serde_json::from_str(r#"{"subsystem":"wan","status":"ok"}"#).unwrap();
        assert!(h.is_ok());
        let bad: HealthSubsystem =
            serde_json::from_str(r#"{"subsystem":"wan","status":"error"}"#).unwrap();
        assert!(!bad.is_ok());
    }
}
