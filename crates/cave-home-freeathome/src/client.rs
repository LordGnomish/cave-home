// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The async SysAP client: real REST + WebSocket I/O over the tested cores.
//!
//! Everything network-shaped lives here and nowhere else. The request shapes,
//! event parsing, state cache and metrics are all pure and tested in their own
//! modules; this module wires them to `reqwest` (REST) and `tokio-tungstenite`
//! (the live WebSocket), so the untestable surface is as thin as possible. The
//! WebSocket read loop ([`run_event_loop`]) is generic over the stream, so the
//! live-update path is exercised in tests with an in-memory message stream.

use std::sync::Arc;
use std::time::Instant;

use futures_util::{Stream, StreamExt as _};
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial};

use crate::config::ClientConfig;
use crate::error::{FreeAtHomeError, Result};
use crate::event::{FreeAtHomeEvent, parse_ws_frame};
use crate::metrics::Metrics;
use crate::model::{ConfigurationResponse, DeviceListResponse};
use crate::rest::{HttpMethod, RestRequest};

/// A connected (or connectable) free@home System Access Point client.
#[derive(Clone)]
pub struct FreeAtHomeClient {
    http: reqwest::Client,
    config: ClientConfig,
    metrics: Arc<Metrics>,
}

impl FreeAtHomeClient {
    /// Build a client from a connection config.
    ///
    /// Honours [`ClientConfig::insecure_tls`] for REST by accepting a
    /// self-signed SysAP certificate.
    pub fn new(config: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(config.insecure_tls())
            .build()
            .map_err(|e| FreeAtHomeError::Http(e.to_string()))?;
        Ok(Self {
            http,
            config,
            metrics: Arc::new(Metrics::new()),
        })
    }

    /// The connection config.
    pub const fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// The shared metrics registry.
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// The `Authorization` header this client sends, if using Basic auth.
    pub fn authorization_header(&self) -> Option<String> {
        self.config.auth().basic_auth_header_value()
    }

    /// Issue one REST request and return the response body.
    pub async fn send_rest(&self, request: RestRequest) -> Result<String> {
        let url = request.url(&self.config.rest_base_url());
        let mut builder = match request.method() {
            HttpMethod::Get => self.http.get(url),
            HttpMethod::Put => {
                let body = request.body().unwrap_or_default().to_string();
                self.http.put(url).body(body)
            }
        };
        if let Some(auth) = self.authorization_header() {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }

        let start = Instant::now();
        let response = builder.send().await.map_err(|e| {
            self.metrics.record_error();
            FreeAtHomeError::Http(e.to_string())
        })?;
        let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.metrics.observe_latency_ms(elapsed);

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            self.metrics.record_auth_failure();
            return Err(FreeAtHomeError::Auth(format!(
                "SysAP rejected credentials ({status})"
            )));
        }
        if !status.is_success() {
            self.metrics.record_error();
            return Err(FreeAtHomeError::Http(format!("SysAP returned {status}")));
        }
        response
            .text()
            .await
            .map_err(|e| FreeAtHomeError::Http(e.to_string()))
    }

    /// Fetch the full SysAP configuration tree.
    pub async fn configuration(&self) -> Result<ConfigurationResponse> {
        ConfigurationResponse::parse(&self.send_rest(RestRequest::Configuration).await?)
    }

    /// Fetch the device list.
    pub async fn device_list(&self) -> Result<DeviceListResponse> {
        DeviceListResponse::parse(&self.send_rest(RestRequest::DeviceList).await?)
    }

    /// Read one datapoint's current wire value.
    pub async fn get_datapoint(
        &self,
        serial: DeviceSerial,
        channel: ChannelId,
        datapoint: DatapointId,
    ) -> Result<String> {
        self.send_rest(RestRequest::get_datapoint(serial, channel, datapoint))
            .await
    }

    /// Write a value to one input datapoint.
    pub async fn set_datapoint(
        &self,
        serial: DeviceSerial,
        channel: ChannelId,
        datapoint: DatapointId,
        value: impl Into<String>,
    ) -> Result<()> {
        self.send_rest(RestRequest::set_datapoint(
            serial, channel, datapoint, value,
        ))
        .await
        .map(|_| ())
    }

    /// Open the live WebSocket, run the read loop, and reconnect with backoff.
    ///
    /// Runs until a fatal error; transient disconnects trigger a backed-off
    /// reconnect. Each datapoint update increments the state-change counter
    /// before `on_event` sees it.
    pub async fn subscribe<F>(&self, mut on_event: F) -> Result<()>
    where
        F: FnMut(FreeAtHomeEvent),
    {
        let mut backoff = crate::reconnect::Backoff::new(
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(60),
        );
        loop {
            match self.connect_stream().await {
                Ok(stream) => {
                    backoff.reset();
                    self.metrics.set_connected(true);
                    let metrics = Arc::clone(&self.metrics);
                    let result = run_event_loop(stream, |ev| {
                        if ev.as_datapoint_update().is_some() {
                            metrics.inc_state_changes();
                        }
                        on_event(ev);
                    })
                    .await;
                    self.metrics.set_connected(false);
                    result?;
                }
                Err(e) => {
                    self.metrics.record_reconnect();
                    tracing::warn!(error = %e, "free@home WebSocket connect failed; backing off");
                    tokio::time::sleep(backoff.next_delay()).await;
                }
            }
        }
    }

    /// Establish the authenticated WebSocket stream.
    async fn connect_stream(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        let mut request = self
            .config
            .ws_url()
            .as_str()
            .into_client_request()
            .map_err(|e| FreeAtHomeError::WebSocket(e.to_string()))?;
        if let Some(auth) = self.authorization_header() {
            let value = HeaderValue::from_str(&auth)
                .map_err(|e| FreeAtHomeError::WebSocket(e.to_string()))?;
            request.headers_mut().insert(AUTHORIZATION, value);
        }

        let connector = if self.config.insecure_tls() {
            Some(Connector::Rustls(insecure_rustls_config()?))
        } else {
            None
        };

        let (stream, _response) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector)
                .await
                .map_err(|e| FreeAtHomeError::WebSocket(e.to_string()))?;
        Ok(stream)
    }
}

/// Drive a WebSocket message stream, decoding text frames into events.
///
/// Generic over the stream so the live-update path is unit-testable. Stops on a
/// `Close` frame or when the stream ends; non-text frames are ignored.
pub async fn run_event_loop<S, F>(mut stream: S, mut on_event: F) -> Result<()>
where
    S: Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    F: FnMut(FreeAtHomeEvent),
{
    while let Some(message) = stream.next().await {
        let message = message.map_err(|e| FreeAtHomeError::WebSocket(e.to_string()))?;
        match message {
            Message::Text(text) => {
                for event in parse_ws_frame(&text)? {
                    on_event(event);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// A rustls config that accepts any server certificate.
///
/// For LAN SysAPs that ship a self-signed certificate; gated behind
/// [`ClientConfig::insecure_tls`]. Certificate pinning is the future hardening.
fn insecure_rustls_config() -> Result<Arc<rustls::ClientConfig>> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::CryptoProvider;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct AcceptAny(Arc<CryptoProvider>);

    impl ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> std::result::Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| FreeAtHomeError::WebSocket(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthMethod;
    use crate::config::ClientConfig;
    use crate::event::FreeAtHomeEvent;
    use futures_util::stream;
    use tokio_tungstenite::tungstenite::Message;

    fn config() -> ClientConfig {
        ClientConfig::new("192.168.1.10", AuthMethod::basic("user", "pass"))
    }

    #[test]
    fn client_builds_from_config() {
        let client = FreeAtHomeClient::new(config()).expect("client");
        assert_eq!(client.config().host(), "192.168.1.10");
    }

    #[test]
    fn client_exposes_authorization_header() {
        let client = FreeAtHomeClient::new(config()).expect("client");
        assert_eq!(
            client.authorization_header(),
            Some("Basic dXNlcjpwYXNz".to_string())
        );
    }

    #[tokio::test]
    async fn event_loop_dispatches_parsed_events() {
        let frame = r#"{ "u": { "datapoints": { "ABB700C12345/ch0000/odp0000": "1" } } }"#;
        let messages = vec![
            Ok(Message::Text(frame.to_string())),
            Ok(Message::Close(None)),
        ];
        let s = stream::iter(messages);
        let mut got = Vec::new();
        run_event_loop(s, |ev| got.push(ev)).await.expect("loop ok");
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], FreeAtHomeEvent::DatapointUpdate(_)));
    }

    #[tokio::test]
    async fn event_loop_ignores_non_text_frames() {
        let messages = vec![Ok(Message::Ping(Vec::new())), Ok(Message::Close(None))];
        let s = stream::iter(messages);
        let mut count = 0usize;
        run_event_loop(s, |_| count += 1).await.expect("loop ok");
        assert_eq!(count, 0);
    }
}
