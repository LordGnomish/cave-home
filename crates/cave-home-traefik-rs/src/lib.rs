// SPDX-License-Identifier: Apache-2.0
//! `cave-home-traefik-rs` — the ingress **routing-decision core**.
//!
//! This crate is part of cave-home's K3s-style orchestration layer
//! (ADR-004, Orchestration Phase 4). It is **infrastructure**: it lives
//! entirely under the hood and produces **no** user-facing strings — per
//! Charter §6.3, the homeowner never sees ingress / router / proxy
//! vocabulary.
//!
//! ## What this crate is
//!
//! The reverse-proxy **routing decision**, modelled std-only and fully
//! deterministic so it is testable without a network, a clock, or global
//! state:
//!
//! * [`rule`] — a hand-rolled parser for the Traefik v3 router-rule grammar
//!   (`Host`, `Path`, `PathPrefix`, `Header`, `Method` + `&&` / `||` / `!`
//!   / parentheses) producing a [`rule::Rule`] AST.
//! * [`matcher`] — evaluates a [`rule::Rule`] against a
//!   [`request::RequestDescriptor`].
//! * [`router`] — the [`router::Router`] model and priority-based
//!   [`router::select`] (default priority = rule length; deterministic
//!   tie-break).
//! * [`loadbalancer`] — a [`loadbalancer::Service`] with a weighted
//!   round-robin / sticky-session, health-aware [`loadbalancer::LoadBalancer`].
//! * [`middleware`] — a typed [`middleware::Middleware`] set and an ordered
//!   [`middleware::MiddlewareChain`] that transforms the request/response.
//! * [`config`] — a validated [`config::DynamicConfig`] snapshot of routers +
//!   services + middlewares with reference checking.
//! * [`ingress`] — translates a Kubernetes [`ingress::Ingress`]
//!   (`networking.k8s.io/v1`) into routers + services (the pure half of the
//!   `kubernetes-ingress` provider).
//! * [`gateway`] — translates a Gateway API [`gateway::HttpRoute`]
//!   (`gateway.networking.k8s.io/v1`) into routers + weighted services (the
//!   pure half of the Gateway provider).
//!
//! ## What is deferred (phase-1b — see `parity.manifest.toml`)
//!
//! The actual TCP/TLS listener, ACME / certificate management, the dynamic
//! provider watchers (file / Kubernetes CRD / Docker), and the byte-level
//! proxying are network-bound and land in a later phase. This crate is the
//! brain those components consult; it intentionally owns no I/O.
//!
//! Port method: **spec-based behavioural reimplementation** of the documented
//! Traefik routing semantics — not a verbatim line-by-line port of Traefik's
//! Go matcher engine.
//!
//! ## Example
//!
//! ```
//! use cave_home_traefik_rs::config::DynamicConfig;
//! use cave_home_traefik_rs::loadbalancer::{LoadBalancer, Server, Service};
//! use cave_home_traefik_rs::request::RequestDescriptor;
//! use cave_home_traefik_rs::router::Router;
//!
//! // A router that sends api traffic for example.com to the `api` service.
//! let router = Router::new("api", "Host(`example.com`) && PathPrefix(`/api`)", "api")
//!     .expect("valid rule");
//! let service = Service::new(
//!     "api",
//!     vec![Server::new("http://10.0.0.2:8080"), Server::new("http://10.0.0.3:8080")],
//!     LoadBalancer::WeightedRoundRobin,
//! );
//!
//! let config = DynamicConfig::build(vec![router], vec![service], vec![])
//!     .expect("valid config");
//!
//! let req = RequestDescriptor::new("GET", "https", "example.com", "/api/users");
//! let route = config.route(&req, None).expect("a route matches");
//! assert_eq!(route.service.name, "api");
//!
//! // The load balancer fans the first two requests across both backends.
//! assert_eq!(route.service.pick_round_robin(0).unwrap().url, "http://10.0.0.2:8080");
//! assert_eq!(route.service.pick_round_robin(1).unwrap().url, "http://10.0.0.3:8080");
//! ```

pub mod config;
pub mod gateway;
pub mod ingress;
pub mod loadbalancer;
pub mod matcher;
pub mod middleware;
pub mod request;
pub mod router;
pub mod rule;

pub use config::{ConfigError, DynamicConfig, Route};
pub use gateway::{
    translate_routes, GatewayError, GatewayTranslation, GwHeaderMatch, GwPathMatch, GwServiceRef,
    HttpBackendRef, HttpRoute, HttpRouteMatch, HttpRouteRule,
};
pub use ingress::{
    translate_ingresses, BackendRef, HttpPath, Ingress, IngressBackend, IngressError, IngressRule,
    IngressTls, PathType, ServicePort, Translation,
};
pub use loadbalancer::{LoadBalancer, Server, Service, StickyPick};
pub use matcher::matches;
pub use middleware::{Applied, Middleware, MiddlewareChain};
pub use request::{RequestDescriptor, ResponseDescriptor};
pub use router::{select, Router};
pub use rule::{parse, ParseError, Rule};

// ── Real runtime (feature = "runtime", default-on) ───────────────────────────
//
// Everything below is the actual HTTP/HTTPS reverse-proxy + Kubernetes-Ingress
// runtime that consults the decision core above: the async TCP/TLS listener,
// the reverse-proxy forwarding engine (retries, circuit breaking), the
// middleware-enforcement layer (auth, rate-limit, compression, CORS), service
// discovery, the ACME client, Prometheus metrics and the dashboard. It is
// gated so that `--no-default-features` still builds the std-only decision core.

/// Bridge between `hyper`/`http` wire types and the core descriptors.
#[cfg(feature = "runtime")]
pub mod wire;

/// Hop-by-hop header stripping + the `X-Forwarded-*` reverse-proxy header set.
#[cfg(feature = "runtime")]
pub mod forwarded;

/// Reverse-proxy upstream request-URI assembly.
#[cfg(feature = "runtime")]
pub mod backend;

/// Circuit breaker for backend forwarding.
#[cfg(feature = "runtime")]
pub mod circuit;

/// Bounded retry policy with exponential backoff.
#[cfg(feature = "runtime")]
pub mod retry;

/// Token-bucket rate limiting.
#[cfg(feature = "runtime")]
pub mod ratelimit;

/// HTTP Basic-auth enforcement.
#[cfg(feature = "runtime")]
pub mod auth;

/// CORS preflight + response decoration.
#[cfg(feature = "runtime")]
pub mod cors;

/// Response compression (gzip / deflate negotiation).
#[cfg(feature = "runtime")]
pub mod compress;

/// TLS termination + SNI certificate resolution.
#[cfg(feature = "runtime")]
pub mod tls;
