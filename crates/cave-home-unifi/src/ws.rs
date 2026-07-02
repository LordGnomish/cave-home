// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The real-time WebSocket subscription engine, shared by all three pillars.
//!
//! UniFi streams live state over WebSockets — the Network event feed
//! (`/wss/s/{site}/events`), the Protect binary update feed
//! (`/proxy/protect/ws/updates`) and the Access notifications feed
//! (`/api/v1/developer/devices/notifications`). They differ only in **URL** and
//! **auth header** (cookie + CSRF for Network/Protect, bearer for Access), so a
//! single engine drives all three:
//!
//! - [`WsRequest`] — the URL + headers, with one builder per pillar (pure,
//!   tested).
//! - [`WsConnection`] / [`WsConnector`] — the async seam (mirroring
//!   [`crate::transport::HttpTransport`]). [`MockWsConnector`] replays canned
//!   frames offline; [`TungsteniteConnector`] is the real `tokio-tungstenite` +
//!   `rustls` client that tolerates the console's self-signed certificate.
//! - [`EventPump`] — drains a connection, counts every frame into [`Metrics`]
//!   per pillar, and forwards frames to an [`mpsc`] channel; the typed
//!   [`EventPump::access_notifications`] decodes each frame to an
//!   [`AccessNotification`] so the intercom path gets structured events.

use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::access::types::AccessNotification;
use crate::access::AccessConfig;
use crate::auth::Session;
use crate::console::Console;
use crate::error::{Result, UnifiError};
use crate::metrics::Metrics;

/// A WebSocket frame in either direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMessage {
    /// A UTF-8 text frame (Network / Access JSON).
    Text(String),
    /// A binary frame (Protect's packed update protocol).
    Binary(Vec<u8>),
    /// A ping (the engine answers with a pong).
    Ping(Vec<u8>),
    /// A pong.
    Pong(Vec<u8>),
    /// The peer closed the stream.
    Close,
}

impl WsMessage {
    /// Whether this frame carries application data (text or binary).
    #[must_use]
    pub fn is_data(&self) -> bool {
        matches!(self, Self::Text(_) | Self::Binary(_))
    }
}

/// The pillar a subscription belongs to (the metrics label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pillar {
    /// Network controller event feed.
    Network,
    /// Protect binary update feed.
    Protect,
    /// Access notifications feed.
    Access,
}

impl Pillar {
    /// The metrics label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Protect => "protect",
            Self::Access => "access",
        }
    }
}

/// A WebSocket connection request: where to connect and with what headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsRequest {
    /// The absolute `ws://`/`wss://` URL.
    pub url: String,
    /// The handshake headers (auth, etc.).
    pub headers: Vec<(String, String)>,
}

impl WsRequest {
    /// The Network controller event-stream subscription, authorized by the
    /// console [`Session`] (cookie + CSRF).
    #[must_use]
    pub fn network_events(console: &Console, site: &str, session: &Session) -> Self {
        let mut headers = Vec::new();
        if let Some(cookie) = session.cookie_header() {
            headers.push(("Cookie".to_string(), cookie));
        }
        Self {
            url: console.network_events_ws_url(site),
            headers,
        }
    }

    /// The Protect binary update-stream subscription, authorized by the console
    /// [`Session`].
    #[must_use]
    pub fn protect_updates(console: &Console, session: &Session) -> Self {
        let mut headers = Vec::new();
        if let Some(cookie) = session.cookie_header() {
            headers.push(("Cookie".to_string(), cookie));
        }
        Self {
            url: console.protect_updates_ws_url(),
            headers,
        }
    }

    /// The Access notifications subscription, authorized by the Access bearer
    /// token.
    #[must_use]
    pub fn access_notifications(config: &AccessConfig) -> Self {
        Self {
            url: config.notifications_ws_url(),
            headers: vec![(
                "Authorization".to_string(),
                format!("Bearer {}", config.token()),
            )],
        }
    }
}

/// An open WebSocket connection: receive frames, send frames.
#[async_trait]
pub trait WsConnection: Send {
    /// Receive the next frame, or `None` when the stream is closed.
    ///
    /// # Errors
    /// [`UnifiError::WebSocket`] on a protocol fault.
    async fn recv(&mut self) -> Result<Option<WsMessage>>;

    /// Send a frame (e.g. a pong, or a keep-alive).
    ///
    /// # Errors
    /// [`UnifiError::WebSocket`] if the frame cannot be sent.
    async fn send(&mut self, message: WsMessage) -> Result<()>;
}

/// Opens [`WsConnection`]s for [`WsRequest`]s.
#[async_trait]
pub trait WsConnector: Send + Sync {
    /// Connect, performing the handshake.
    ///
    /// # Errors
    /// [`UnifiError::WebSocket`] on a handshake / transport fault.
    async fn connect(&self, request: WsRequest) -> Result<Box<dyn WsConnection>>;
}

/// A deterministic, offline [`WsConnection`] that replays queued frames then
/// reports the stream closed, recording everything sent.
pub struct MockWsConnection {
    incoming: VecDeque<Result<Option<WsMessage>>>,
    sent: Arc<Mutex<Vec<WsMessage>>>,
}

impl MockWsConnection {
    /// A connection that will yield `frames` (each as one `recv`), then `None`.
    #[must_use]
    pub fn new(frames: Vec<WsMessage>) -> Self {
        let mut incoming: VecDeque<Result<Option<WsMessage>>> =
            frames.into_iter().map(|f| Ok(Some(f))).collect();
        incoming.push_back(Ok(None));
        Self {
            incoming,
            sent: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A handle to the frames the driver has sent back.
    #[must_use]
    pub fn sent_handle(&self) -> Arc<Mutex<Vec<WsMessage>>> {
        Arc::clone(&self.sent)
    }
}

#[async_trait]
impl WsConnection for MockWsConnection {
    async fn recv(&mut self) -> Result<Option<WsMessage>> {
        self.incoming.pop_front().unwrap_or(Ok(None))
    }

    async fn send(&mut self, message: WsMessage) -> Result<()> {
        self.sent.lock().push(message);
        Ok(())
    }
}

/// A [`WsConnector`] that hands out pre-built [`MockWsConnection`]s.
pub struct MockWsConnector {
    queue: Mutex<VecDeque<Vec<WsMessage>>>,
    requests: Mutex<Vec<WsRequest>>,
}

impl MockWsConnector {
    /// A connector that yields one connection per queued frame-set.
    #[must_use]
    pub fn new(connections: Vec<Vec<WsMessage>>) -> Self {
        Self {
            queue: Mutex::new(connections.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// The requests this connector has been asked to open.
    #[must_use]
    pub fn requests(&self) -> Vec<WsRequest> {
        self.requests.lock().clone()
    }
}

#[async_trait]
impl WsConnector for MockWsConnector {
    async fn connect(&self, request: WsRequest) -> Result<Box<dyn WsConnection>> {
        self.requests.lock().push(request);
        let frames = self
            .queue
            .lock()
            .pop_front()
            .ok_or_else(|| UnifiError::WebSocket("no mock connection queued".into()))?;
        Ok(Box::new(MockWsConnection::new(frames)))
    }
}

/// Drains a [`WsConnection`], counting frames into [`Metrics`] and forwarding
/// the data frames to a channel.
pub struct EventPump {
    metrics: Arc<Metrics>,
}

impl EventPump {
    /// A pump reporting into `metrics`.
    #[must_use]
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self { metrics }
    }

    /// Run the receive loop until the stream closes: every data frame is
    /// counted (per `pillar`) and sent to `out`; pings are answered with pongs.
    /// Returns the number of data frames forwarded.
    ///
    /// # Errors
    /// A transport / protocol fault, or [`UnifiError::WebSocket`] if `out` is
    /// dropped by the receiver.
    pub async fn run(
        &self,
        mut conn: Box<dyn WsConnection>,
        pillar: Pillar,
        out: mpsc::Sender<WsMessage>,
    ) -> Result<u64> {
        let mut count = 0u64;
        loop {
            match conn.recv().await? {
                None | Some(WsMessage::Close) => break,
                Some(WsMessage::Ping(payload)) => {
                    conn.send(WsMessage::Pong(payload)).await?;
                }
                Some(WsMessage::Pong(_)) => {}
                Some(data) => {
                    self.metrics.record_ws_event(pillar.label());
                    count += 1;
                    out.send(data)
                        .await
                        .map_err(|_| UnifiError::WebSocket("event receiver dropped".into()))?;
                }
            }
        }
        Ok(count)
    }

    /// Drive an Access notifications connection, decoding each text frame into an
    /// [`AccessNotification`] forwarded to `out`. Non-text / unparseable frames
    /// are counted but skipped. Returns the number of notifications forwarded.
    ///
    /// # Errors
    /// A transport / protocol fault, or if `out` is dropped.
    pub async fn access_notifications(
        &self,
        mut conn: Box<dyn WsConnection>,
        out: mpsc::Sender<AccessNotification>,
    ) -> Result<u64> {
        let mut count = 0u64;
        loop {
            match conn.recv().await? {
                None | Some(WsMessage::Close) => break,
                Some(WsMessage::Ping(p)) => conn.send(WsMessage::Pong(p)).await?,
                Some(WsMessage::Text(text)) => {
                    self.metrics.record_ws_event(Pillar::Access.label());
                    if let Ok(note) = AccessNotification::parse(&text) {
                        count += 1;
                        out.send(note).await.map_err(|_| {
                            UnifiError::WebSocket("notification receiver dropped".into())
                        })?;
                    }
                }
                Some(_) => {}
            }
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// The real tokio-tungstenite + rustls connector.
// ---------------------------------------------------------------------------

pub use real::TungsteniteConnector;

mod real {
    use super::{Result, UnifiError, WsConnection, WsConnector, WsMessage, WsRequest};
    use async_trait::async_trait;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use tokio::net::TcpStream;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

    /// The real WebSocket connector: `tokio-tungstenite` over `rustls`,
    /// configured to accept the console's self-signed certificate (the same
    /// trust posture as [`crate::transport::ReqwestTransport`]).
    #[derive(Clone)]
    pub struct TungsteniteConnector {
        tls: Arc<rustls::ClientConfig>,
    }

    impl std::fmt::Debug for TungsteniteConnector {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TungsteniteConnector").finish_non_exhaustive()
        }
    }

    impl TungsteniteConnector {
        /// Build a connector that trusts the console's self-signed certificate.
        #[must_use]
        pub fn new() -> Self {
            let tls = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(no_verify::NoVerify::new()))
                .with_no_client_auth();
            Self { tls: Arc::new(tls) }
        }
    }

    impl Default for TungsteniteConnector {
        fn default() -> Self {
            Self::new()
        }
    }

    /// The live connection: a tungstenite stream mapped to [`WsMessage`].
    pub struct TungsteniteConnection {
        stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    }

    fn to_ws(message: Message) -> Option<WsMessage> {
        match message {
            Message::Text(t) => Some(WsMessage::Text(t.to_string())),
            Message::Binary(b) => Some(WsMessage::Binary(b.to_vec())),
            Message::Ping(p) => Some(WsMessage::Ping(p.to_vec())),
            Message::Pong(p) => Some(WsMessage::Pong(p.to_vec())),
            Message::Close(_) => Some(WsMessage::Close),
            Message::Frame(_) => None,
        }
    }

    #[async_trait]
    impl WsConnection for TungsteniteConnection {
        async fn recv(&mut self) -> Result<Option<WsMessage>> {
            loop {
                match self.stream.next().await {
                    None => return Ok(None),
                    Some(Ok(msg)) => {
                        if let Some(m) = to_ws(msg) {
                            return Ok(Some(m));
                        }
                        // a raw frame we don't surface; keep reading
                    }
                    Some(Err(e)) => {
                        return Err(UnifiError::WebSocket(e.to_string()));
                    }
                }
            }
        }

        async fn send(&mut self, message: WsMessage) -> Result<()> {
            let msg = match message {
                WsMessage::Text(t) => Message::Text(t.into()),
                WsMessage::Binary(b) => Message::Binary(b.into()),
                WsMessage::Ping(p) => Message::Ping(p.into()),
                WsMessage::Pong(p) => Message::Pong(p.into()),
                WsMessage::Close => Message::Close(None),
            };
            self.stream
                .send(msg)
                .await
                .map_err(|e| UnifiError::WebSocket(e.to_string()))
        }
    }

    #[async_trait]
    impl WsConnector for TungsteniteConnector {
        async fn connect(&self, request: WsRequest) -> Result<Box<dyn WsConnection>> {
            let mut req = request
                .url
                .as_str()
                .into_client_request()
                .map_err(|e| UnifiError::WebSocket(format!("bad ws url: {e}")))?;
            for (name, value) in &request.headers {
                let header = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|e| UnifiError::WebSocket(format!("bad header name: {e}")))?;
                let val = HeaderValue::from_str(value)
                    .map_err(|e| UnifiError::WebSocket(format!("bad header value: {e}")))?;
                req.headers_mut().insert(header, val);
            }
            let connector = Connector::Rustls(Arc::clone(&self.tls));
            let (stream, _resp) = tokio_tungstenite::connect_async_tls_with_config(
                req,
                None,
                false,
                Some(connector),
            )
            .await
            .map_err(|e| UnifiError::WebSocket(e.to_string()))?;
            Ok(Box::new(TungsteniteConnection { stream }))
        }
    }

    /// A rustls certificate verifier that accepts any server certificate — the
    /// LAN-appliance trust posture (no public CA brokers a self-signed console
    /// cert; Charter §9 keeps us off the cloud that otherwise would). No
    /// `unsafe` is involved.
    mod no_verify {
        use rustls::client::danger::{
            HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
        };
        use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature};
        use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
        use rustls::{DigitallySignedStruct, Error, SignatureScheme};

        #[derive(Debug)]
        pub struct NoVerify {
            schemes: Vec<SignatureScheme>,
        }

        impl NoVerify {
            pub fn new() -> Self {
                Self {
                    schemes: ring::default_provider()
                        .signature_verification_algorithms
                        .supported_schemes(),
                }
            }
        }

        impl ServerCertVerifier for NoVerify {
            fn verify_server_cert(
                &self,
                _end_entity: &CertificateDer<'_>,
                _intermediates: &[CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: UnixTime,
            ) -> Result<ServerCertVerified, Error> {
                Ok(ServerCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                message: &[u8],
                cert: &CertificateDer<'_>,
                dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, Error> {
                verify_tls12_signature(
                    message,
                    cert,
                    dss,
                    &ring::default_provider().signature_verification_algorithms,
                )
            }

            fn verify_tls13_signature(
                &self,
                message: &[u8],
                cert: &CertificateDer<'_>,
                dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, Error> {
                verify_tls13_signature(
                    message,
                    cert,
                    dss,
                    &ring::default_provider().signature_verification_algorithms,
                )
            }

            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                self.schemes.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_ws_request_carries_cookie() {
        let console = Console::unifi_os("10.0.0.1");
        let mut session = Session::new();
        session.ingest_response_headers([("set-cookie", "TOKEN=h.p.s; Path=/")]);
        let req = WsRequest::network_events(&console, "default", &session);
        assert_eq!(
            req.url,
            "wss://10.0.0.1:443/proxy/network/wss/s/default/events"
        );
        let cookie = req
            .headers
            .iter()
            .find(|(k, _)| k == "Cookie")
            .map(|(_, v)| v.as_str());
        assert!(cookie.unwrap().contains("TOKEN="));
    }

    #[test]
    fn access_ws_request_carries_bearer() {
        let cfg = AccessConfig::new("nas", "TOK");
        let req = WsRequest::access_notifications(&cfg);
        assert_eq!(
            req.url,
            "wss://nas:12445/api/v1/developer/devices/notifications"
        );
        assert_eq!(
            req.headers,
            vec![("Authorization".to_string(), "Bearer TOK".to_string())]
        );
    }

    #[test]
    fn protect_ws_request_url() {
        let req = WsRequest::protect_updates(&Console::unifi_os("h"), &Session::new());
        assert_eq!(req.url, "wss://h:443/proxy/protect/ws/updates");
    }

    #[tokio::test]
    async fn pump_forwards_data_and_counts_per_pillar() {
        let metrics = Arc::new(Metrics::new());
        let conn = Box::new(MockWsConnection::new(vec![
            WsMessage::Text("a".into()),
            WsMessage::Binary(vec![1, 2]),
            WsMessage::Pong(vec![]),
        ]));
        let (tx, mut rx) = mpsc::channel(8);
        let pump = EventPump::new(Arc::clone(&metrics));
        let n = pump.run(conn, Pillar::Protect, tx).await.unwrap();
        assert_eq!(n, 2); // pong is not data
        assert_eq!(rx.recv().await, Some(WsMessage::Text("a".into())));
        assert_eq!(rx.recv().await, Some(WsMessage::Binary(vec![1, 2])));
        assert!(metrics
            .render_prometheus()
            .contains("unifi_ws_events_total{pillar=\"protect\"} 2"));
    }

    #[tokio::test]
    async fn pump_answers_ping_with_pong() {
        let metrics = Arc::new(Metrics::new());
        let conn = MockWsConnection::new(vec![WsMessage::Ping(vec![9])]);
        let sent = conn.sent_handle();
        let (tx, _rx) = mpsc::channel(8);
        let pump = EventPump::new(metrics);
        pump.run(Box::new(conn), Pillar::Network, tx).await.unwrap();
        assert_eq!(sent.lock().as_slice(), &[WsMessage::Pong(vec![9])]);
    }

    #[tokio::test]
    async fn access_notifications_decode_intercom_call() {
        let metrics = Arc::new(Metrics::new());
        let frame = r#"{"event":"access.remote_view","data":{"door":{"name":"Front door"}}}"#;
        let conn = Box::new(MockWsConnection::new(vec![
            WsMessage::Text(frame.into()),
            WsMessage::Text("garbage".into()),
        ]));
        let (tx, mut rx) = mpsc::channel(8);
        let pump = EventPump::new(Arc::clone(&metrics));
        let n = pump.access_notifications(conn, tx).await.unwrap();
        assert_eq!(n, 1); // garbage skipped
        let note = rx.recv().await.unwrap();
        assert!(note.is_intercom_call());
        assert_eq!(note.door_name.as_deref(), Some("Front door"));
        // both text frames were counted as ws events even though one didn't parse
        assert!(metrics
            .render_prometheus()
            .contains("unifi_ws_events_total{pillar=\"access\"} 2"));
    }

    #[tokio::test]
    async fn mock_connector_records_request_and_yields_connection() {
        let connector = MockWsConnector::new(vec![vec![WsMessage::Text("x".into())]]);
        let req = WsRequest::access_notifications(&AccessConfig::new("h", "T"));
        let conn = connector.connect(req.clone()).await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let pump = EventPump::new(Arc::new(Metrics::new()));
        pump.run(conn, Pillar::Access, tx).await.unwrap();
        assert_eq!(rx.recv().await, Some(WsMessage::Text("x".into())));
        assert_eq!(connector.requests(), vec![req]);
    }

    #[test]
    fn real_connector_constructs() {
        // Build the real connector (exercises the rustls dangerous config path);
        // an actual connect needs a live console, covered by integration only.
        let _c = TungsteniteConnector::new();
    }
}
