// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi Protect REST surface (bootstrap, cameras, events, live URL).
//!
//! Protect is behind the console session (`/proxy/protect/api/...`), so
//! [`ProtectApi`] borrows the same [`ConsoleClient`] the Network surface uses
//! and speaks plain JSON (no `{meta,data}` envelope — that is Network-only). It
//! reads the bootstrap (NVR + cameras), lists cameras lowered to the domain
//! model, derives the **live RTSPS stream URL** for a camera, and reads the
//! event log as domain detections. The update WebSocket URL comes from
//! [`crate::console::Console::protect_updates_ws_url`].

use cave_home_unifi_protect::{DetectionEvent, ProtectCamera};

use super::types::{WireBootstrap, WireCamera, WireEvent, WireNvr};
use crate::client::ConsoleClient;
use crate::error::{Result, UnifiError};
use crate::transport::HttpTransport;

/// The bootstrap projection cave-home keeps: the NVR, the cameras (domain), and
/// the update cursor.
#[derive(Debug, Clone)]
pub struct Bootstrap {
    /// The NVR record.
    pub nvr: WireNvr,
    /// The adopted cameras, lowered to the domain model.
    pub cameras: Vec<ProtectCamera>,
    /// The `lastUpdateId` cursor the update WebSocket resumes from.
    pub last_update_id: Option<String>,
}

/// The Protect API, bound to one [`ConsoleClient`].
pub struct ProtectApi<'a, T: HttpTransport> {
    client: &'a ConsoleClient<T>,
}

impl<'a, T: HttpTransport> ProtectApi<'a, T> {
    /// Bind to a console client.
    #[must_use]
    pub fn new(client: &'a ConsoleClient<T>) -> Self {
        Self { client }
    }

    /// Fetch and parse the raw bootstrap document.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors.
    pub async fn bootstrap_raw(&self) -> Result<WireBootstrap> {
        let url = self.client.console().protect_url("bootstrap");
        self.client.get_json(url, "protect/bootstrap").await
    }

    /// Fetch the bootstrap, projected to the domain model.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors.
    pub async fn bootstrap(&self) -> Result<Bootstrap> {
        let raw = self.bootstrap_raw().await?;
        Ok(Bootstrap {
            nvr: raw.nvr,
            cameras: raw
                .cameras
                .into_iter()
                .map(WireCamera::into_domain)
                .collect(),
            last_update_id: raw.last_update_id,
        })
    }

    /// List the cameras (`/api/cameras`), lowered to the domain model.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors.
    pub async fn cameras(&self) -> Result<Vec<ProtectCamera>> {
        let url = self.client.console().protect_url("cameras");
        let wires: Vec<WireCamera> = self.client.get_json(url, "protect/cameras").await?;
        Ok(wires.into_iter().map(WireCamera::into_domain).collect())
    }

    /// The live RTSPS stream URL for a camera, derived from the bootstrap's
    /// channel alias and the console host.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or [`UnifiError::InvalidArgument`] if
    /// the camera is unknown / advertises no RTSPS alias.
    pub async fn camera_live_url(&self, camera_id: &str) -> Result<String> {
        let raw = self.bootstrap_raw().await?;
        let host = self.client.console().host().to_string();
        let cam = raw
            .cameras
            .into_iter()
            .find(|c| c.id == camera_id)
            .ok_or_else(|| UnifiError::InvalidArgument(format!("unknown camera {camera_id}")))?;
        cam.live_rtsps_url(&host).ok_or_else(|| {
            UnifiError::InvalidArgument(format!("camera {camera_id} has no RTSPS stream"))
        })
    }

    /// Read the event log in `[start, end)` (unix ms), as domain detections.
    /// Events without a camera (which the domain model requires) are skipped.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors.
    pub async fn events(&self, start_ms: u64, end_ms: u64) -> Result<Vec<DetectionEvent>> {
        let url = self
            .client
            .console()
            .protect_url(&format!("events?start={start_ms}&end={end_ms}"));
        let wires: Vec<WireEvent> = self.client.get_json(url, "protect/events").await?;
        Ok(wires
            .into_iter()
            .filter_map(WireEvent::into_detection)
            .collect())
    }

    /// Read a camera's recordings (recorded events) in `[start, end)` (unix ms).
    ///
    /// # Errors
    /// Transport / HTTP / decode errors.
    pub async fn recordings(
        &self,
        camera_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Vec<DetectionEvent>> {
        let url = self.client.console().protect_url(&format!(
            "events?start={start_ms}&end={end_ms}&cameras={camera_id}"
        ));
        let wires: Vec<WireEvent> = self.client.get_json(url, "protect/recordings").await?;
        Ok(wires
            .into_iter()
            .filter(|e| e.camera.as_deref() == Some(camera_id))
            .filter_map(WireEvent::into_detection)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credentials;
    use crate::console::Console;
    use crate::transport::{HttpResponse, MockTransport};
    use cave_home_unifi_protect::SmartDetectType;

    fn client_returning(body: &[u8]) -> ConsoleClient<MockTransport> {
        let t = MockTransport::new();
        t.push(HttpResponse::json(200, body.to_vec()));
        ConsoleClient::new(
            Console::unifi_os("10.0.0.3"),
            t,
            Credentials::api_key("KEY"),
        )
    }

    #[tokio::test]
    async fn bootstrap_projects_cameras_and_url_is_proxy_prefixed() {
        let client = client_returning(
            br#"{"lastUpdateId":"u9","nvr":{"id":"n","name":"NVR","version":"4"},
                 "cameras":[{"id":"c1","name":"Cam","mac":"m","state":"CONNECTED"}]}"#,
        );
        let api = ProtectApi::new(&client);
        let bs = api.bootstrap().await.unwrap();
        assert_eq!(bs.nvr.name, "NVR");
        assert_eq!(bs.cameras.len(), 1);
        assert_eq!(bs.last_update_id.as_deref(), Some("u9"));
        let req = client.transport().last_request().unwrap();
        assert_eq!(
            req.url,
            "https://10.0.0.3:443/proxy/protect/api/bootstrap"
        );
    }

    #[tokio::test]
    async fn cameras_lower_to_domain() {
        let client = client_returning(
            br#"[{"id":"c1","name":"Driveway","mac":"m","isConnected":true,
                  "featureFlags":{"smartDetectTypes":["vehicle"]},
                  "recordingSettings":{"mode":"always"}}]"#,
        );
        let api = ProtectApi::new(&client);
        let cams = api.cameras().await.unwrap();
        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].name, "Driveway");
        assert!(cams[0].supports(SmartDetectType::Vehicle));
    }

    #[tokio::test]
    async fn camera_live_url_built_from_bootstrap_alias_and_host() {
        let client = client_returning(
            br#"{"cameras":[
                {"id":"c1","name":"Front","mac":"m","channels":[{"id":0,"rtspAlias":"AbC123"}]}
            ]}"#,
        );
        let api = ProtectApi::new(&client);
        let url = api.camera_live_url("c1").await.unwrap();
        assert_eq!(url, "rtsps://10.0.0.3:7441/AbC123?enableSrtp");
    }

    #[tokio::test]
    async fn camera_live_url_unknown_camera_errors() {
        let client = client_returning(br#"{"cameras":[]}"#);
        let api = ProtectApi::new(&client);
        let err = api.camera_live_url("nope").await.unwrap_err();
        assert!(matches!(err, UnifiError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn events_map_to_detections_skipping_cameraless() {
        let client = client_returning(
            br#"[
                {"id":"e1","type":"smartDetectZone","camera":"c1","score":80,
                 "start":1,"smartDetectTypes":["person"]},
                {"id":"e2","type":"motion","score":5}
            ]"#,
        );
        let api = ProtectApi::new(&client);
        let dets = api.events(0, 9_999).await.unwrap();
        assert_eq!(dets.len(), 1);
        assert!(dets[0].has_type(SmartDetectType::Person));
    }

    #[tokio::test]
    async fn recordings_filter_by_camera() {
        let client = client_returning(
            br#"[
                {"id":"e1","type":"smartDetectZone","camera":"c1","score":80,"start":1,
                 "smartDetectTypes":["person"]},
                {"id":"e2","type":"smartDetectZone","camera":"c2","score":80,"start":1,
                 "smartDetectTypes":["vehicle"]}
            ]"#,
        );
        let api = ProtectApi::new(&client);
        let recs = api.recordings("c1", 0, 9_999).await.unwrap();
        assert_eq!(recs.len(), 1);
        assert!(recs[0].has_type(SmartDetectType::Person));
        let req = client.transport().last_request().unwrap();
        assert!(req.url.contains("cameras=c1"));
    }
}
