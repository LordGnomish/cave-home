// SPDX-License-Identifier: Apache-2.0
//! End-to-end integration tests: a real HTTP request travels through the proxy
//! to a real backend over loopback TCP, exercising routing, the reverse-proxy
//! forwarding engine, X-Forwarded-* injection, middleware redirect short-circuit
//! and metrics.
#![cfg(feature = "runtime")]

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};

use cave_home_traefik_rs::config::DynamicConfig;
use cave_home_traefik_rs::controller::ConfigHolder;
use cave_home_traefik_rs::loadbalancer::{LoadBalancer, Server, Service};
use cave_home_traefik_rs::metrics::Metrics;
use cave_home_traefik_rs::middleware::{Middleware, MiddlewareChain};
use cave_home_traefik_rs::router::Router;
use cave_home_traefik_rs::server::Proxy;

/// Spawn a loopback HTTP backend that echoes the path, the inbound Host header
/// and the X-Forwarded-For it received. Returns its address.
async fn spawn_echo_backend() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let svc = service_fn(|req: Request<Incoming>| async move {
                    let path = req.uri().path().to_string();
                    let host = header(&req, "host");
                    let xff = header(&req, "x-forwarded-for");
                    let resp = Response::builder()
                        .status(200)
                        .header("x-saw-host", host)
                        .header("x-saw-xff", xff)
                        .body(Full::new(Bytes::from(format!("echo {path}"))))
                        .unwrap();
                    Ok::<_, Infallible>(resp)
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, svc)
                    .await;
            });
        }
    });
    addr
}

fn header<B>(req: &Request<B>, name: &str) -> String {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("none")
        .to_string()
}

/// Issue a GET through the proxy and return (status, headers, body).
async fn proxy_get(
    proxy: std::net::SocketAddr,
    host: &str,
    path: &str,
) -> (StatusCode, Vec<(String, String)>, String) {
    let stream = TcpStream::connect(proxy).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = Request::builder()
        .uri(path)
        .header("host", host)
        .body(Empty::<Bytes>::new())
        .unwrap();
    let resp = sender.send_request(req).await.unwrap();
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).to_string())
}

fn start_proxy(config: DynamicConfig, metrics: Arc<Metrics>) -> (std::net::SocketAddr, Arc<Proxy>) {
    let proxy = Arc::new(Proxy::new(Arc::new(ConfigHolder::new(config)), metrics));
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let addr = std_listener.local_addr().unwrap();
    let listener = TcpListener::from_std(std_listener).unwrap();
    tokio::spawn(proxy.clone().serve_http(listener));
    (addr, proxy)
}

#[tokio::test]
async fn proxies_request_to_backend_with_forwarded_headers() {
    let backend = spawn_echo_backend().await;
    let config = DynamicConfig::build(
        vec![Router::new("web", "Host(`proxy.test`)", "backend").unwrap()],
        vec![Service::new(
            "backend",
            vec![Server::new(&format!("http://{backend}"))],
            LoadBalancer::WeightedRoundRobin,
        )],
        vec![],
    )
    .unwrap();
    let metrics = Arc::new(Metrics::new());
    let (proxy_addr, _proxy) = start_proxy(config, metrics.clone());

    let (status, headers, body) = proxy_get(proxy_addr, "proxy.test", "/hello").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "echo /hello");

    let saw = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    // passHostHeader: backend sees the original Host.
    assert_eq!(saw("x-saw-host").as_deref(), Some("proxy.test"));
    // the proxy injected X-Forwarded-For with the loopback client.
    assert_eq!(saw("x-saw-xff").as_deref(), Some("127.0.0.1"));

    // metrics recorded a 200.
    let rendered = metrics.render();
    assert!(rendered.contains("traefik_requests_total"));
    assert!(rendered.contains("code=\"200\""));
}

#[tokio::test]
async fn unknown_host_returns_404() {
    let backend = spawn_echo_backend().await;
    let config = DynamicConfig::build(
        vec![Router::new("web", "Host(`proxy.test`)", "backend").unwrap()],
        vec![Service::new(
            "backend",
            vec![Server::new(&format!("http://{backend}"))],
            LoadBalancer::WeightedRoundRobin,
        )],
        vec![],
    )
    .unwrap();
    let (proxy_addr, _proxy) = start_proxy(config, Arc::new(Metrics::new()));

    let (status, _, _) = proxy_get(proxy_addr, "nobody.test", "/").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn middleware_redirect_short_circuits_without_backend() {
    // No backend needed: the redirect middleware terminates the request.
    let router = Router::new("secure", "Host(`secure.test`)", "backend")
        .unwrap()
        .with_middlewares(&["to-https"]);
    let redirect = MiddlewareChain::new(vec![Middleware::RedirectScheme {
        scheme: "https".to_string(),
        port: None,
        permanent: false,
    }]);
    let config = DynamicConfig::build(
        vec![router],
        vec![Service::new(
            "backend",
            vec![Server::new("http://127.0.0.1:1")],
            LoadBalancer::WeightedRoundRobin,
        )],
        vec![("to-https".to_string(), redirect)],
    )
    .unwrap();
    let (proxy_addr, _proxy) = start_proxy(config, Arc::new(Metrics::new()));

    let (status, headers, _) = proxy_get(proxy_addr, "secure.test", "/dashboard").await;
    assert_eq!(status, StatusCode::FOUND);
    let location = headers.iter().find(|(n, _)| n == "location").map(|(_, v)| v.clone());
    assert_eq!(location.as_deref(), Some("https://secure.test/dashboard"));
}
