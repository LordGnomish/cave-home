// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The rate-limited, transport-injected Fleet API request model.
//!
//! The client owns the *decisions* — per-endpoint rate limiting (1 req / 30 s),
//! exponential 429 back-off, auth-header injection and status→error mapping —
//! over an injected [`HttpTransport`] and an injected [`Clock`]. The real
//! `reqwest`/TLS transport is the only deferred piece (Phase 1b); the tests
//! drive everything through [`MockTransport`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::endpoints::{ApiRequest, EnergyEndpoint, HttpMethod};
use super::Region;
use crate::error::{Result, TeslaError};

// ----------------------------------------------------------------------------
// Clock
// ----------------------------------------------------------------------------

/// A monotonic-ish millisecond clock, injected so the rate limiter and retry
/// loop are testable without real time.
pub trait Clock: Send + Sync {
    /// Milliseconds since some fixed epoch.
    fn now_millis(&self) -> u64;
}

/// The production clock — wall time since the Unix epoch, in milliseconds.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// A test clock the caller advances by hand.
#[derive(Debug)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    /// Start at `start` milliseconds.
    #[must_use]
    pub const fn new(start: u64) -> Self {
        Self(AtomicU64::new(start))
    }

    /// Advance the clock by `delta` milliseconds.
    pub fn advance(&self, delta: u64) {
        self.0.fetch_add(delta, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

// ----------------------------------------------------------------------------
// Rate limiter
// ----------------------------------------------------------------------------

/// The verdict from a [`RateLimiter::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDecision {
    /// The request may proceed now (and the limiter has recorded it).
    Ready,
    /// The caller must wait this many milliseconds before retrying.
    Wait(u64),
}

/// A per-endpoint minimum-interval gate. Tesla's energy endpoints allow roughly
/// one request every 30 seconds; this enforces that key-by-key.
#[derive(Debug)]
pub struct RateLimiter {
    interval_ms: u64,
    last: Mutex<HashMap<String, u64>>,
}

impl RateLimiter {
    /// A limiter with an explicit minimum interval in milliseconds.
    #[must_use]
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            last: Mutex::new(HashMap::new()),
        }
    }

    /// A limiter with an interval expressed in seconds.
    #[must_use]
    pub fn with_interval_secs(secs: u64) -> Self {
        Self::new(secs.saturating_mul(1_000))
    }

    /// Check whether a request to `key` may proceed at `now_ms`. On
    /// [`RateDecision::Ready`] the call is recorded so the next one is gated.
    #[must_use]
    pub fn check(&self, key: &str, now_ms: u64) -> RateDecision {
        let mut last = self.last.lock();
        match last.get(key) {
            Some(&prev) if now_ms.saturating_sub(prev) < self.interval_ms => {
                RateDecision::Wait(self.interval_ms - (now_ms.saturating_sub(prev)))
            }
            _ => {
                last.insert(key.to_string(), now_ms);
                RateDecision::Ready
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Back-off
// ----------------------------------------------------------------------------

/// Exponential back-off for retryable failures (HTTP 429 / 5xx).
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    base_ms: u64,
    cap_ms: u64,
}

impl Backoff {
    /// A back-off doubling from `base_ms`, clamped at `cap_ms`.
    #[must_use]
    pub const fn new(base_ms: u64, cap_ms: u64) -> Self {
        Self { base_ms, cap_ms }
    }

    /// The delay before retry `attempt` (0-indexed): `base * 2^attempt`, capped.
    #[must_use]
    pub const fn delay_for_attempt(&self, attempt: u32) -> u64 {
        match self.base_ms.checked_shl(attempt) {
            Some(d) if d <= self.cap_ms => d,
            _ => self.cap_ms,
        }
    }
}

// ----------------------------------------------------------------------------
// Transport
// ----------------------------------------------------------------------------

/// A transport-ready HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The method.
    pub method: HttpMethod,
    /// The absolute URL.
    pub url: String,
    /// Request headers.
    pub headers: Vec<(String, String)>,
    /// The request body, if any.
    pub body: Option<String>,
}

impl HttpRequest {
    /// A header-less GET, mostly for tests.
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }
}

/// A transport response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body.
    pub body: String,
}

/// The pluggable HTTP transport. The production impl (reqwest + rustls) is
/// Phase-1b; everything in this crate is tested against [`MockTransport`].
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Perform the request.
    ///
    /// # Errors
    /// [`TeslaError::Transport`] on a socket/TLS failure (a non-2xx HTTP status
    /// is *not* an error here — it is returned as an [`HttpResponse`]).
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse>;
}

/// An in-memory transport for tests and the integration suite.
///
/// Matches requests against URL substrings, supports a FIFO queue of sequenced
/// responses, logs every request, and can be forced to fail (to exercise the
/// cache path).
#[derive(Debug, Default)]
pub struct MockTransport {
    routes: Mutex<Vec<(String, HttpResponse)>>,
    queue: Mutex<Vec<HttpResponse>>,
    /// Every request seen, in order.
    pub requests: Mutex<Vec<HttpRequest>>,
    failure: Mutex<Option<String>>,
}

impl MockTransport {
    /// An empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: respond to any URL containing `matches` with `status`/`body`.
    #[must_use]
    pub fn route(self, matches: impl Into<String>, status: u16, body: impl Into<String>) -> Self {
        self.routes.lock().push((
            matches.into(),
            HttpResponse {
                status,
                body: body.into(),
            },
        ));
        self
    }

    /// Queue a one-shot response, consumed FIFO before routes are consulted.
    pub fn push_response(&self, status: u16, body: impl Into<String>) {
        self.queue.lock().push(HttpResponse {
            status,
            body: body.into(),
        });
    }

    /// Force every subsequent `send` to fail with a transport error (or clear
    /// it with `None`).
    pub fn set_failure(&self, msg: Option<String>) {
        *self.failure.lock() = msg;
    }

    /// How many requests have been sent.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.requests.lock().len()
    }
}

#[async_trait]
impl HttpTransport for MockTransport {
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse> {
        self.requests.lock().push(req.clone());
        let failure = self.failure.lock().clone();
        if let Some(msg) = failure {
            return Err(TeslaError::Transport(msg));
        }
        {
            let mut queue = self.queue.lock();
            if !queue.is_empty() {
                return Ok(queue.remove(0));
            }
        }
        let routes = self.routes.lock();
        routes
            .iter()
            .find(|(m, _)| req.url.contains(m.as_str()))
            .map(|(_, r)| r.clone())
            .ok_or_else(|| TeslaError::Http {
                status: 404,
                body: format!("no mock route for {}", req.url),
            })
    }
}

// ----------------------------------------------------------------------------
// Fleet client
// ----------------------------------------------------------------------------

/// The Fleet API client: an injected transport + region + per-endpoint rate
/// limiter + an optional bearer token.
#[derive(Debug)]
pub struct FleetClient<T: HttpTransport> {
    transport: T,
    region: Region,
    rate: RateLimiter,
    token: Option<String>,
}

impl<T: HttpTransport> FleetClient<T> {
    /// A client with the default 30-second per-endpoint rate limit.
    #[must_use]
    pub fn new(transport: T, region: Region) -> Self {
        Self::with_rate_interval(transport, region, 30)
    }

    /// A client with an explicit rate-limit interval in seconds (0 disables it).
    #[must_use]
    pub fn with_rate_interval(transport: T, region: Region, interval_secs: u64) -> Self {
        Self {
            transport,
            region,
            rate: RateLimiter::with_interval_secs(interval_secs),
            token: None,
        }
    }

    /// Attach a bearer access token.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Borrow the underlying transport (used by tests to inspect requests).
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// Perform one call to `endpoint` at `now_ms`, enforcing the rate limit,
    /// injecting the auth header and mapping non-2xx onto [`TeslaError`].
    ///
    /// # Errors
    /// [`TeslaError::RateLimited`] if the endpoint was called too recently,
    /// [`TeslaError::Unauthorized`]/[`TeslaError::Http`] for error statuses, or
    /// a transport error.
    pub async fn call(&self, endpoint: &EnergyEndpoint<'_>, now_ms: u64) -> Result<HttpResponse> {
        let req = endpoint.request();
        if let RateDecision::Wait(ms) = self.rate.check(&req.path, now_ms) {
            return Err(TeslaError::RateLimited {
                retry_after_secs: ms.div_ceil(1_000),
            });
        }
        let http = self.build_request(&req);
        let resp = self.transport.send(http).await?;
        if (200..300).contains(&resp.status) {
            Ok(resp)
        } else {
            Err(TeslaError::from_status(resp.status, &resp.body))
        }
    }

    fn build_request(&self, req: &ApiRequest) -> HttpRequest {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if let Some(token) = &self.token {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        if req.body.is_some() {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }
        HttpRequest {
            method: req.method,
            url: req.full_url(self.region),
            headers,
            body: req.body.clone(),
        }
    }
}

/// Call `endpoint`, retrying retryable failures with exponential back-off.
///
/// Rate limits and 5xx are retried up to `max_retries` times. The clock drives
/// the rate limiter; [`tokio::time::sleep`] drives the back-off (virtual under
/// a paused test runtime).
///
/// # Errors
/// The last error if every attempt fails, or the first terminal error.
pub async fn send_with_retry<T: HttpTransport>(
    client: &FleetClient<T>,
    endpoint: &EnergyEndpoint<'_>,
    clock: &dyn Clock,
    backoff: Backoff,
    max_retries: u32,
) -> Result<HttpResponse> {
    let mut attempt = 0u32;
    loop {
        match client.call(endpoint, clock.now_millis()).await {
            Ok(resp) => return Ok(resp),
            Err(e) if e.is_retryable() && attempt < max_retries => {
                let delay = backoff.delay_for_attempt(attempt);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::endpoints::{EnergyEndpoint, HttpMethod};
    use super::super::Region;
    use super::*;

    #[test]
    fn rate_limiter_first_request_is_ready() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
    }

    #[test]
    fn rate_limiter_throttles_a_burst() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
        // Same endpoint, 1s later: must wait ~29s.
        assert_eq!(rl.check("live", 1_000), RateDecision::Wait(29_000));
        assert_eq!(rl.check("live", 29_999), RateDecision::Wait(1));
    }

    #[test]
    fn rate_limiter_recovers_after_interval() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
        assert_eq!(rl.check("live", 30_000), RateDecision::Ready);
    }

    #[test]
    fn rate_limiter_keys_are_independent() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
        assert_eq!(rl.check("info", 0), RateDecision::Ready);
    }

    #[test]
    fn backoff_doubles_up_to_a_cap() {
        let b = Backoff::new(1_000, 60_000);
        assert_eq!(b.delay_for_attempt(0), 1_000);
        assert_eq!(b.delay_for_attempt(1), 2_000);
        assert_eq!(b.delay_for_attempt(2), 4_000);
        assert_eq!(b.delay_for_attempt(6), 60_000); // 64s clamped to 60s cap
        assert_eq!(b.delay_for_attempt(100), 60_000);
    }

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(5);
        assert_eq!(c.now_millis(), 5);
        c.advance(10);
        assert_eq!(c.now_millis(), 15);
    }

    #[tokio::test]
    async fn mock_transport_routes_and_logs_requests() {
        let t = MockTransport::new().route("/live_status", 200, r#"{"ok":true}"#);
        let resp = t
            .send(HttpRequest::get("https://x/api/1/energy_sites/1/live_status"))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(t.request_count(), 1);
    }

    #[tokio::test]
    async fn client_call_returns_ok_body() {
        let t = MockTransport::new().route("/live_status", 200, r#"{"response":{}}"#);
        let client = FleetClient::new(t, Region::Europe).with_token("AT-123");
        let resp = client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn client_call_injects_bearer_header() {
        let t = MockTransport::new().route("/live_status", 200, "{}");
        let client = FleetClient::new(t, Region::Europe).with_token("AT-123");
        client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap();
        let reqs = client.transport().requests.lock();
        let auth = reqs[0]
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"));
        assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer AT-123"));
    }

    #[tokio::test]
    async fn client_call_maps_non_2xx_to_error() {
        let t = MockTransport::new().route("/live_status", 500, "boom");
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        let err = client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap_err();
        assert!(matches!(err, TeslaError::Http { status: 500, .. }));
    }

    #[tokio::test]
    async fn client_call_enforces_rate_limit() {
        let t = MockTransport::new().route("/live_status", 200, "{}");
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap();
        // Second call to the same endpoint 1s later is throttled.
        let err = client
            .call(&EnergyEndpoint::LiveStatus(1), 1_000)
            .await
            .unwrap_err();
        assert!(matches!(err, TeslaError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn client_post_sets_method_and_body() {
        let t = MockTransport::new().route("/backup", 200, "{}");
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        client
            .call(&EnergyEndpoint::SetBackupReserve { site_id: 1, percent: 30 }, 0)
            .await
            .unwrap();
        let reqs = client.transport().requests.lock();
        assert_eq!(reqs[0].method, HttpMethod::Post);
        assert!(reqs[0].body.as_deref().unwrap().contains("backup_reserve_percent"));
    }

    #[tokio::test(start_paused = true)]
    async fn send_with_retry_recovers_from_429() {
        // First response 429, then 200. Rate limit disabled (interval 0) so the
        // retry exercises the back-off path, not the limiter.
        let t = MockTransport::new();
        t.push_response(429, "slow down");
        t.push_response(200, r#"{"response":{}}"#);
        let client = FleetClient::with_rate_interval(t, Region::Europe, 0).with_token("AT");
        let clock = ManualClock::new(0);
        let resp = send_with_retry(
            &client,
            &EnergyEndpoint::LiveStatus(1),
            &clock,
            Backoff::new(10, 1_000),
            3,
        )
        .await
        .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(client.transport().request_count(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn send_with_retry_gives_up_after_max() {
        let t = MockTransport::new().route("/live_status", 503, "down");
        let client = FleetClient::with_rate_interval(t, Region::Europe, 0).with_token("AT");
        let clock = ManualClock::new(0);
        let err = send_with_retry(
            &client,
            &EnergyEndpoint::LiveStatus(1),
            &clock,
            Backoff::new(10, 1_000),
            2,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, TeslaError::Http { status: 503, .. }));
        // initial try + 2 retries
        assert_eq!(client.transport().request_count(), 3);
    }
}
