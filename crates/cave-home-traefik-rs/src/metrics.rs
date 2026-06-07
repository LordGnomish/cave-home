// SPDX-License-Identifier: Apache-2.0
//! Prometheus metrics for the proxy.
//!
//! Spec basis: Traefik exposes request counters, in-flight gauges and request
//! duration histograms (per entrypoint / router / service) in the Prometheus
//! exposition format. This registers an equivalent set on a
//! `prometheus-client` registry and renders the text format.

use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::registry::Registry;

/// The label set for a handled request.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RequestLabels {
    /// The router that matched (or `"-"` when none did).
    pub router: String,
    /// The service the request was forwarded to (or `"-"`).
    pub service: String,
    /// The request method.
    pub method: String,
    /// The response status code, as a string.
    pub code: String,
}

/// The proxy's metric set.
#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    requests_total: Family<RequestLabels, Counter>,
    request_duration_seconds: Histogram,
    open_connections: Gauge,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    /// Build and register the metric set.
    #[must_use]
    pub fn new() -> Self {
        unimplemented!()
    }

    /// Count a handled request.
    pub fn record_request(&self, router: &str, service: &str, method: &str, code: u16) {
        unimplemented!()
    }

    /// Observe a request's duration in seconds.
    pub fn observe_duration(&self, seconds: f64) {
        unimplemented!()
    }

    /// Increment the in-flight connection gauge.
    pub fn inc_open(&self) {
        unimplemented!()
    }

    /// Decrement the in-flight connection gauge.
    pub fn dec_open(&self) {
        unimplemented!()
    }

    /// Render the registry in the Prometheus text exposition format.
    #[must_use]
    pub fn render(&self) -> String {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_render_with_labels() {
        let m = Metrics::new();
        m.record_request("api", "api-svc", "GET", 200);
        m.record_request("api", "api-svc", "GET", 200);
        let out = m.render();
        assert!(out.contains("traefik_requests_total"));
        assert!(out.contains("router=\"api\""));
        assert!(out.contains("code=\"200\""));
        // Two identical requests => counter value 2.
        assert!(out.contains("} 2"));
    }

    #[test]
    fn duration_histogram_is_exposed() {
        let m = Metrics::new();
        m.observe_duration(0.012);
        let out = m.render();
        assert!(out.contains("traefik_request_duration_seconds"));
        assert!(out.contains("_bucket"));
    }

    #[test]
    fn open_connections_gauge_tracks_inflight() {
        let m = Metrics::new();
        m.inc_open();
        m.inc_open();
        m.dec_open();
        let out = m.render();
        assert!(out.contains("traefik_open_connections 1"));
    }
}
