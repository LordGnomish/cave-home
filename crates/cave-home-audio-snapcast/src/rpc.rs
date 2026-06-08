//! A typed model of the JSON-RPC 2.0 messages the engine would exchange with a
//! Snapcast server — *without* any transport.
//!
//! Modelled from the public Snapcast control-protocol description (the JSON-RPC
//! 2.0 method names and parameter shapes such as `Client.SetVolume`,
//! `Group.SetStream`, and the `Client.OnConnect` notification). This module owns
//! only the message structure and its JSON round-trip via the std-only
//! [`crate::json`] helper; it never opens a socket. The TCP transport and the
//! live notification stream are network-bound and deferred to Phase 1b (see the
//! parity manifest, ADR-020). Snapcast source was NOT read.

use std::collections::BTreeMap;

use crate::json::Json;

/// A control request the engine sends to the server.
///
/// The `id` correlates a request with its response (JSON-RPC 2.0). `method` is
/// a dotted control verb; `params` is its argument object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Correlation id.
    pub id: i64,
    /// The dotted method name, e.g. `"Client.SetVolume"`.
    pub method: String,
    /// Method parameters as an ordered key/value object.
    pub params: BTreeMap<String, Json>,
}

impl Request {
    /// Build a request with no parameters.
    #[must_use]
    pub fn new(id: i64, method: impl Into<String>) -> Self {
        Self {
            id,
            method: method.into(),
            params: BTreeMap::new(),
        }
    }

    /// Add a parameter (builder style).
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: Json) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    /// Encode to a JSON-RPC 2.0 request string.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut obj = BTreeMap::new();
        obj.insert("id".to_string(), Json::Int(self.id));
        obj.insert("jsonrpc".to_string(), Json::Str("2.0".to_string()));
        obj.insert("method".to_string(), Json::Str(self.method.clone()));
        obj.insert("params".to_string(), Json::Obj(self.params.clone()));
        Json::Obj(obj).to_string()
    }

    /// Decode a JSON-RPC 2.0 request string.
    ///
    /// # Errors
    /// Returns a message if the input is not a JSON object with an integer `id`
    /// and a string `method`. A missing `params` is treated as an empty object.
    pub fn from_json(input: &str) -> Result<Self, String> {
        let v = Json::parse(input)?;
        let id = v
            .get("id")
            .and_then(Json::as_int)
            .ok_or_else(|| "missing integer `id`".to_string())?;
        let method = v
            .get("method")
            .and_then(Json::as_str)
            .ok_or_else(|| "missing string `method`".to_string())?
            .to_string();
        let params = match v.get("params") {
            Some(Json::Obj(m)) => m.clone(),
            Some(_) => return Err("`params` must be an object".to_string()),
            None => BTreeMap::new(),
        };
        Ok(Self { id, method, params })
    }
}

/// A server-pushed notification (no `id`, JSON-RPC 2.0 "notification" form),
/// e.g. `Client.OnConnect`, `Client.OnVolumeChanged`, `Group.OnStreamChanged`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The dotted notification method.
    pub method: String,
    /// Notification parameters.
    pub params: BTreeMap<String, Json>,
}

impl Notification {
    /// Build a notification with no parameters.
    #[must_use]
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            params: BTreeMap::new(),
        }
    }

    /// Add a parameter (builder style).
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: Json) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    /// Encode to a JSON-RPC 2.0 notification string (no `id`).
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut obj = BTreeMap::new();
        obj.insert("jsonrpc".to_string(), Json::Str("2.0".to_string()));
        obj.insert("method".to_string(), Json::Str(self.method.clone()));
        obj.insert("params".to_string(), Json::Obj(self.params.clone()));
        Json::Obj(obj).to_string()
    }

    /// Decode a JSON-RPC 2.0 notification.
    ///
    /// # Errors
    /// Returns a message if the input is not an object with a string `method`,
    /// or if it carries an `id` (which would make it a request, not a
    /// notification).
    pub fn from_json(input: &str) -> Result<Self, String> {
        let v = Json::parse(input)?;
        if v.get("id").is_some() {
            return Err("notification must not carry an `id`".to_string());
        }
        let method = v
            .get("method")
            .and_then(Json::as_str)
            .ok_or_else(|| "missing string `method`".to_string())?
            .to_string();
        let params = match v.get("params") {
            Some(Json::Obj(m)) => m.clone(),
            Some(_) => return Err("`params` must be an object".to_string()),
            None => BTreeMap::new(),
        };
        Ok(Self { method, params })
    }
}

/// The control method names the engine models, as documented public Snapcast
/// JSON-RPC verbs.
///
/// Kept as named constants so callers never hand-type a dotted string (and so a
/// typo is a compile error, not a silent no-op).
pub mod method {
    /// `Client.SetVolume` — set a client's volume + mute.
    pub const CLIENT_SET_VOLUME: &str = "Client.SetVolume";
    /// `Client.SetLatency` — set a client's latency trim.
    pub const CLIENT_SET_LATENCY: &str = "Client.SetLatency";
    /// `Client.SetName` — rename a client.
    pub const CLIENT_SET_NAME: &str = "Client.SetName";
    /// `Group.SetStream` — point a group at a stream.
    pub const GROUP_SET_STREAM: &str = "Group.SetStream";
    /// `Group.SetMute` — mute/unmute a group.
    pub const GROUP_SET_MUTE: &str = "Group.SetMute";
    /// `Group.SetClients` — set a group's membership.
    pub const GROUP_SET_CLIENTS: &str = "Group.SetClients";
    /// `Server.GetStatus` — fetch the whole topology tree.
    pub const SERVER_GET_STATUS: &str = "Server.GetStatus";
    /// `Client.OnConnect` notification — a client (re)appeared.
    pub const CLIENT_ON_CONNECT: &str = "Client.OnConnect";
    /// `Client.OnVolumeChanged` notification.
    pub const CLIENT_ON_VOLUME_CHANGED: &str = "Client.OnVolumeChanged";
    /// `Group.OnStreamChanged` notification.
    pub const GROUP_ON_STREAM_CHANGED: &str = "Group.OnStreamChanged";
}

/// Build the canonical `Client.SetVolume` request (the documented param shape is
/// `{"id": <client>, "volume": {"muted": <bool>, "percent": <0..=100>}}`).
#[must_use]
pub fn client_set_volume(req_id: i64, client_id: &str, percent: u8, muted: bool) -> Request {
    let mut volume = BTreeMap::new();
    volume.insert("muted".to_string(), Json::Bool(muted));
    volume.insert("percent".to_string(), Json::Int(i64::from(percent)));
    Request::new(req_id, method::CLIENT_SET_VOLUME)
        .with("id", Json::Str(client_id.to_string()))
        .with("volume", Json::Obj(volume))
}

/// Build a `Group.SetStream` request
/// (`{"id": <group>, "stream_id": <stream>}`).
#[must_use]
pub fn group_set_stream(req_id: i64, group_id: &str, stream_id: &str) -> Request {
    Request::new(req_id, method::GROUP_SET_STREAM)
        .with("id", Json::Str(group_id.to_string()))
        .with("stream_id", Json::Str(stream_id.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn request_round_trips() {
        let req = client_set_volume(8, "c1", 70, false);
        let wire = req.to_json();
        let back = Request::from_json(&wire).expect("decode");
        assert_eq!(back, req);
        assert_eq!(back.id, 8);
        assert_eq!(back.method, "Client.SetVolume");
    }

    #[test]
    fn request_wire_is_jsonrpc_2() {
        let wire = group_set_stream(3, "g1", "spotify").to_json();
        let v = Json::parse(&wire).expect("parse");
        assert_eq!(v.get("jsonrpc").and_then(Json::as_str), Some("2.0"));
        assert_eq!(v.get("method").and_then(Json::as_str), Some("Group.SetStream"));
        assert_eq!(
            v.get("params").and_then(|p| p.get("stream_id")).and_then(Json::as_str),
            Some("spotify")
        );
    }

    #[test]
    fn set_volume_param_shape() {
        let req = client_set_volume(1, "c1", 55, true);
        let vol = req.params.get("volume").expect("volume param");
        assert_eq!(vol.get("percent").and_then(Json::as_int), Some(55));
        assert_eq!(vol.get("muted").and_then(Json::as_bool), Some(true));
    }

    #[test]
    fn notification_round_trips_and_has_no_id() {
        let note = Notification::new(method::CLIENT_ON_CONNECT)
            .with("id", Json::Str("c1".to_string()));
        let wire = note.to_json();
        // The "id" here is a *param* (which client connected), not a JSON-RPC id.
        let v = Json::parse(&wire).expect("parse");
        assert!(v.get("id").is_none(), "notification must have no top-level id");
        let back = Notification::from_json(&wire).expect("decode");
        assert_eq!(back, note);
    }

    #[test]
    fn notification_rejects_request_with_id() {
        let req_wire = client_set_volume(8, "c1", 70, false).to_json();
        assert!(Notification::from_json(&req_wire).is_err());
    }

    #[test]
    fn request_rejects_missing_method_or_id() {
        assert!(Request::from_json("{\"id\":1}").is_err());
        assert!(Request::from_json("{\"method\":\"X\"}").is_err());
        assert!(Request::from_json("{\"id\":1,\"method\":42}").is_err());
    }

    #[test]
    fn request_rejects_non_object_params() {
        assert!(Request::from_json("{\"id\":1,\"method\":\"X\",\"params\":[1,2]}").is_err());
    }

    #[test]
    fn request_missing_params_defaults_empty() {
        let r = Request::from_json("{\"id\":1,\"method\":\"Server.GetStatus\"}").expect("ok");
        assert!(r.params.is_empty());
    }
}
