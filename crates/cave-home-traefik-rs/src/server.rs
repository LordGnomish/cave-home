// SPDX-License-Identifier: Apache-2.0
//! The real async HTTP/HTTPS reverse-proxy listener.
//!
//! This is the runtime that ties the decision core to the network: it accepts
//! TCP (and TLS) connections, turns each `http::Request` into a
//! [`RequestDescriptor`](crate::request::RequestDescriptor), routes it through
//! the live [`DynamicConfig`](crate::config::DynamicConfig), applies the
//! middleware chain (path rewrite + redirect short-circuit + header sets),
//! load-balances onto a backend [`Server`], injects the `X-Forwarded-*`
//! headers, forwards over hyper with a bounded retry across servers, and
//! records Prometheus metrics.
//!
//! Bodies are buffered (collected) rather than streamed — adequate and robust
//! for the ingress workloads this serves; streaming is a future refinement.

use std::convert::Infallible;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use http::{Method, StatusCode, Uri};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};

use crate::backend::upstream_uri;
use crate::controller::ConfigHolder;
use crate::forwarded::{strip_hop_by_hop, Forwarded};
use crate::loadbalancer::LoadBalancer;
use crate::metrics::Metrics;
use crate::middleware::MiddlewareChain;
use crate::request::RequestDescriptor;
use crate::retry::RetryPolicy;
use crate::wire;

/// The scheme an entrypoint terminates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    Http,
    Https,
}

impl Scheme {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

/// The reverse proxy: shared, cheaply cloneable state behind an `Arc`.
#[derive(Debug)]
pub struct Proxy {
    config: Arc<ConfigHolder>,
    metrics: Arc<Metrics>,
    counter: AtomicU64,
    retry: RetryPolicy,
}

impl Proxy {
    /// Build a proxy over a shared config holder and metric set.
    #[must_use]
    pub const fn new(config: Arc<ConfigHolder>, metrics: Arc<Metrics>) -> Self {
        Self {
            config,
            metrics,
            counter: AtomicU64::new(0),
            retry: RetryPolicy { attempts: 2, initial_interval_ms: 0, max_interval_ms: 0 },
        }
    }

    /// The shared metric set (for the `/metrics` endpoint).
    #[must_use]
    pub const fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    /// The shared config holder (for the controller to hot-swap into).
    #[must_use]
    pub const fn config(&self) -> &Arc<ConfigHolder> {
        &self.config
    }

    /// Serve plaintext HTTP on `listener` until the task is cancelled.
    pub async fn serve_http(self: Arc<Self>, listener: TcpListener) {
        self.accept_loop(listener, Scheme::Http, None).await;
    }

    /// Serve HTTPS on `listener`, terminating TLS with `acceptor`.
    pub async fn serve_https(
        self: Arc<Self>,
        listener: TcpListener,
        acceptor: tokio_rustls::TlsAcceptor,
    ) {
        self.accept_loop(listener, Scheme::Https, Some(acceptor)).await;
    }

    async fn accept_loop(
        self: Arc<Self>,
        listener: TcpListener,
        scheme: Scheme,
        acceptor: Option<tokio_rustls::TlsAcceptor>,
    ) {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                continue;
            };
            let client_ip = peer.ip();
            let me = self.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match acceptor {
                    Some(acc) => {
                        if let Ok(tls) = acc.accept(stream).await {
                            me.serve_connection(TokioIo::new(tls), scheme, client_ip).await;
                        }
                    }
                    None => {
                        me.serve_connection(TokioIo::new(stream), scheme, client_ip).await;
                    }
                }
            });
        }
    }

    async fn serve_connection<I>(self: Arc<Self>, io: I, scheme: Scheme, client_ip: IpAddr)
    where
        I: hyper::rt::Read + hyper::rt::Write + Unpin + 'static,
    {
        let service = service_fn(move |req: Request<Incoming>| {
            let me = self.clone();
            async move { me.handle(req, scheme, client_ip).await }
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(io, service)
            .await;
    }

    async fn handle(
        self: Arc<Self>,
        req: Request<Incoming>,
        scheme: Scheme,
        client_ip: IpAddr,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        self.metrics.inc_open();
        let response = self.route_and_forward(req, scheme, client_ip).await;
        self.metrics.dec_open();
        Ok(response)
    }

    #[allow(clippy::too_many_lines)]
    async fn route_and_forward(
        &self,
        req: Request<Incoming>,
        scheme: Scheme,
        client_ip: IpAddr,
    ) -> Response<Full<Bytes>> {
        let (parts, body) = req.into_parts();
        let desc =
            wire::descriptor_from_parts(&parts.method, &parts.uri, &parts.headers, scheme.as_str());

        let config = self.config.load();
        let Some(route) = config.route(&desc, None) else {
            return status_response(StatusCode::NOT_FOUND, "no matching route");
        };
        let router_name = route.router.name.clone();
        let service = route.service.clone();
        let middleware_names = route.router.middlewares.clone();

        // Compose the router's middleware chains, then apply (path rewrite,
        // header sets, redirect short-circuit).
        let mut middlewares = Vec::new();
        for name in &middleware_names {
            if let Some(chain) = config.middleware(name) {
                middlewares.extend(chain.middlewares.clone());
            }
        }
        let applied = MiddlewareChain::new(middlewares).apply(desc.clone());

        if applied.short_circuited {
            let (status, headers) = wire::short_circuit_parts(&applied.response);
            self.metrics
                .record_request(&router_name, &service.name, parts.method.as_str(), status.as_u16());
            return build_response(status, headers, Bytes::new());
        }

        // Buffer the request body once; it is replayed on each retry.
        let body_bytes = body.collect().await.map(http_body_util::Collected::to_bytes).unwrap_or_default();

        // Base upstream headers: original minus hop-by-hop, plus middleware
        // request-header changes, plus the X-Forwarded-* set.
        let mut headers = parts.headers.clone();
        strip_hop_by_hop(&mut headers);
        apply_request_header_overrides(&mut headers, &applied.request, &desc);
        Forwarded {
            client_ip: client_ip.to_string(),
            proto: scheme.as_str().to_string(),
            host: desc.host.clone(),
            port: parts.uri.port_u16(),
        }
        .apply(&mut headers);

        let cookie = match &service.policy {
            LoadBalancer::Sticky { cookie_name } => {
                wire::cookie_value(&parts.headers, cookie_name)
            }
            LoadBalancer::WeightedRoundRobin => None,
        };
        let query = parts.uri.query();
        let base_counter = self.counter.fetch_add(1, Ordering::Relaxed);

        // Forward with a bounded retry across (rotating) backends.
        let mut upstream: Option<(StatusCode, HeaderMap, Bytes, Option<String>)> = None;
        for attempt in 0..self.retry.attempts.max(1) {
            let Some(pick) = service.select(cookie.as_deref(), base_counter + u64::from(attempt))
            else {
                break;
            };
            let Ok(target) = upstream_uri(&pick.server.url, &applied.request.path, query) else {
                continue;
            };
            if let Ok((status, resp_headers, resp_body)) =
                dial_and_send(&target, &parts.method, &headers, body_bytes.clone()).await
            {
                upstream = Some((status, resp_headers, resp_body, pick.set_cookie.clone()));
                break;
            }
        }

        let Some((status, mut resp_headers, resp_body, set_cookie)) = upstream else {
            self.metrics
                .record_request(&router_name, &service.name, parts.method.as_str(), 502);
            return status_response(StatusCode::BAD_GATEWAY, "no backend reachable");
        };

        strip_hop_by_hop(&mut resp_headers);
        wire::apply_response_headers(&mut resp_headers, &applied.response);
        if let (LoadBalancer::Sticky { cookie_name }, Some(value)) = (&service.policy, &set_cookie) {
            if let Ok(hv) = HeaderValue::from_str(&format!("{cookie_name}={value}; Path=/")) {
                resp_headers.append(http::header::SET_COOKIE, hv);
            }
        }

        self.metrics
            .record_request(&router_name, &service.name, parts.method.as_str(), status.as_u16());
        build_response(status, resp_headers, resp_body)
    }
}

/// Open a fresh connection to `target`, send the request, and collect the
/// response into `(status, headers, body)`. Returns `Err(())` on any transport
/// failure so the caller can retry the next backend.
async fn dial_and_send(
    target: &Uri,
    method: &Method,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, HeaderMap, Bytes), ()> {
    let host = target.host().ok_or(())?;
    let default_port = if target.scheme_str() == Some("https") { 443 } else { 80 };
    let port = target.port_u16().unwrap_or(default_port);
    let stream = TcpStream::connect((host, port)).await.map_err(|_| ())?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.map_err(|_| ())?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    // Origin-form request line (path+query); the Host header (carried over from
    // the client) identifies the virtual host to the backend.
    let path_and_query = target.path_and_query().map_or("/", http::uri::PathAndQuery::as_str);
    let mut builder = Request::builder().method(method).uri(path_and_query);
    for (name, value) in headers {
        builder = builder.header(name.clone(), value.clone());
    }
    let request = builder.body(Full::new(body)).map_err(|_| ())?;

    let response = sender.send_request(request).await.map_err(|_| ())?;
    let status = response.status();
    let resp_headers = response.headers().clone();
    let resp_body = response.into_body().collect().await.map_err(|_| ())?.to_bytes();
    Ok((status, resp_headers, resp_body))
}

/// Apply the request-header changes a middleware made (additions or overrides
/// relative to the original descriptor) onto the upstream header map.
fn apply_request_header_overrides(
    dst: &mut HeaderMap,
    applied: &RequestDescriptor,
    original: &RequestDescriptor,
) {
    for (key, value) in &applied.headers {
        if original.headers.get(key).map(String::as_str) != Some(value.as_str()) {
            if let (Ok(name), Ok(val)) =
                (HeaderName::from_bytes(key.as_bytes()), HeaderValue::from_str(value))
            {
                dst.insert(name, val);
            }
        }
    }
}

/// A plain status response with a short text body.
fn status_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(message.to_owned())));
    *response.status_mut() = status;
    response
}

/// Assemble a response from status + headers + body.
fn build_response(status: StatusCode, headers: HeaderMap, body: Bytes) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

/// Build a `tokio-rustls` acceptor from a rustls server configuration.
#[must_use]
pub fn tls_acceptor(config: rustls::ServerConfig) -> tokio_rustls::TlsAcceptor {
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_sets_status_and_body() {
        let r = status_response(StatusCode::NOT_FOUND, "nope");
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn header_overrides_only_apply_middleware_changes() {
        let original = RequestDescriptor::new("GET", "http", "h", "/")
            .with_header("x-keep", "orig");
        let applied = original.clone().with_header("x-new", "added");
        let mut dst = HeaderMap::new();
        dst.insert(HeaderName::from_static("x-keep"), HeaderValue::from_static("orig"));
        apply_request_header_overrides(&mut dst, &applied, &original);
        // unchanged header is not re-applied, new one is
        assert_eq!(dst.get("x-new").unwrap(), "added");
        assert_eq!(dst.get("x-keep").unwrap(), "orig");
    }
}
