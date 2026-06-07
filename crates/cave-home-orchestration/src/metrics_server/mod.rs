//! The in-process port of kubernetes-sigs/metrics-server's decision core.
//!
//! This is the cluster-wide **resource-metrics pipeline** (node + pod CPU /
//! memory) that K3s bundles and that backs `kubectl top`.
//!
//! Upstream metrics-server is a standalone Deployment that (1) scrapes every
//! node's kubelet for a usage *summary*, (2) keeps the **last two** cumulative
//! samples per object so it can derive a CPU usage *rate*, and (3) serves those
//! as `metrics.k8s.io/v1beta1` `NodeMetrics` / `PodMetrics` through the
//! Kubernetes **aggregation layer** (a registered `APIService`). cave-home runs
//! that same pipeline *in process* inside the one binary (Charter §5, ADR-004),
//! so this module is the pure-logic reimplementation of its scrape → store →
//! serve decision spine, reproduced from the public metrics-server architecture
//! (Apache-2.0, ADR-002), **not** transcribed verbatim from Go.
//!
//! # Modules
//!
//! - [`quantity`] — the `resource.Quantity` slice metrics-server needs: CPU in
//!   canonical `DecimalSI`, memory in `BinarySI`, plus the kubectl-style
//!   round-up display values.
//! - [`summary`] — the kubelet `/stats/summary` data model + the
//!   node/pod/container [`summary::MetricsPoint`] extraction (`decode.go`).
//! - [`store`] — the in-memory ring-buffer point storage and the cumulative-
//!   counter → CPU-rate computation (`storage/point.go`), with counter-reset and
//!   zero-window rejection.
//! - [`scraper`] — the scrape scheduling decision (interval gating) and the
//!   per-node latency / error accounting that the observability track exports.
//!
//! # Scope (honest)
//!
//! Pure logic, std-only: no HTTP, no TLS, no clock. The caller supplies the
//! scraped summary bytes and the monotonic timestamps; this module makes the
//! *decisions* metrics-server makes — what to scrape and when, how to derive the
//! rate, how to shape the API objects, what to register. The actual kubelet
//! HTTPS scrape transport, the aggregation-layer serving, and TLS are
//! runtime-bound (ADR-004 phase-1b) and enumerated in `parity.manifest.toml`.

pub mod quantity;
pub mod scraper;
pub mod store;
pub mod summary;

pub use quantity::{Quantity, ResourceList};
pub use scraper::{ScrapeConfig, ScrapeFailure, ScrapeOutcome, ScrapeResult, Scraper};
pub use store::{PointRing, RateError, Storage, Usage};
pub use summary::{MetricsPoint, Summary};
