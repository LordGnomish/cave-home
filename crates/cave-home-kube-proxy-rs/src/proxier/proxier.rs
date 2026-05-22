// SPDX-License-Identifier: Apache-2.0
//! Composition root: the `Proxier` glues `ServiceCache` + `EndpointSliceCache`
//! + `build_proxy_rules` + `IptablesExecutor` together and runs the reconciler.
//!
//! Upstream: `pkg/proxy/iptables/proxier.go` `Proxier` struct.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

use crate::api::WatchEvent;
use crate::cache::endpointslice_cache::EndpointSliceCache;
use crate::cache::service_cache::ServiceCache;
use crate::cache::source::EventSource;
use crate::iptables::errors::ProxierError;
use crate::iptables::executor::IptablesExecutor;
use crate::iptables::rules_builder::{build_proxy_rules, BuildInput};
use crate::iptables::types::Table;
use crate::proxier::reconciler::BoundedFrequencyConfig;

/// Configurable knobs for a `Proxier` instance.
#[derive(Debug, Clone)]
pub struct ProxierConfig {
    pub cluster_cidr: Option<String>,
    pub frequency: BoundedFrequencyConfig,
}

impl Default for ProxierConfig {
    fn default() -> Self {
        Self {
            cluster_cidr: None,
            frequency: BoundedFrequencyConfig::default(),
        }
    }
}

/// The Phase-1 Proxier — shareable (Arc inside) so the reconciler loop
/// can be `tokio::spawn`-ed and the caller can still call `sync_once()`.
#[derive(Clone)]
pub struct Proxier {
    inner: Arc<ProxierInner>,
}

struct ProxierInner {
    cfg: ProxierConfig,
    exec: Arc<dyn IptablesExecutor>,
    services: ServiceCache,
    endpoints: EndpointSliceCache,
    /// Receiver bound at construction time so events are NOT lost between
    /// successive `sync_once` calls (which each only `try_recv`).
    rx: Mutex<mpsc::UnboundedReceiver<WatchEvent>>,
}

impl Proxier {
    #[must_use]
    pub fn new(
        cfg: ProxierConfig,
        src: Arc<dyn EventSource>,
        exec: Arc<dyn IptablesExecutor>,
    ) -> Self {
        let rx = src.stream();
        Self {
            inner: Arc::new(ProxierInner {
                cfg,
                exec,
                services: ServiceCache::new(),
                endpoints: EndpointSliceCache::new(),
                rx: Mutex::new(rx),
            }),
        }
    }

    /// Drain any currently-available events from the source, snapshot the
    /// caches, build rules, and feed the executor exactly once. This is
    /// the building block both `run_until` and tests use.
    pub async fn sync_once(&self) -> Result<(), ProxierError> {
        // -- 1. drain pending watch events into caches --------------------
        {
            let mut rx = self.inner.rx.lock().await;
            while let Ok(ev) = rx.try_recv() {
                self.apply_event(ev);
            }
        }

        // -- 2. clear dirty (we're about to sync) -------------------------
        let _ = self.inner.services.take_dirty();
        let _ = self.inner.endpoints.take_dirty();

        // -- 3. snapshot + build rules ------------------------------------
        let services = self.inner.services.snapshot();
        let endpoints_by_service = self.inner.endpoints.snapshot();
        let input = BuildInput {
            services,
            endpoints_by_service,
            cluster_cidr: self.inner.cfg.cluster_cidr.clone(),
        };
        let rules = build_proxy_rules(&input);

        // -- 4. render to iptables-restore text ---------------------------
        let mut text = String::with_capacity(rules.len() * 80);
        for r in rules.iter().filter(|r| r.table == Table::Nat) {
            text.push_str(&r.text);
            text.push('\n');
        }

        // -- 5. push to executor ------------------------------------------
        self.inner.exec.restore(&text).await
    }

    /// Apply a single watch event to the caches.
    fn apply_event(&self, ev: WatchEvent) {
        match ev {
            WatchEvent::ServiceAdded(s) => self.inner.services.add(s),
            WatchEvent::ServiceModified(s) => self.inner.services.modify(s),
            WatchEvent::ServiceDeleted(s) => self.inner.services.delete(&s),
            WatchEvent::EndpointSliceAdded(s) => self.inner.endpoints.add(s),
            WatchEvent::EndpointSliceModified(s) => self.inner.endpoints.modify(s),
            WatchEvent::EndpointSliceDeleted(s) => self.inner.endpoints.delete(&s),
        }
    }

    /// Reconciler loop that runs until `deadline` elapses. Mirrors
    /// upstream `Proxier.SyncLoop` but with a finite deadline so tests
    /// can call it directly. Production callers pass `Duration::MAX`.
    pub async fn run_until(&self, deadline: Duration) -> Result<(), ProxierError> {
        let started = Instant::now();
        let mut last_sync = Instant::now()
            .checked_sub(self.inner.cfg.frequency.resync_period)
            .unwrap_or_else(Instant::now);

        loop {
            if started.elapsed() >= deadline {
                return Ok(());
            }

            // Wait for either an event, a periodic resync timer, or the
            // overall deadline to elapse. We use a short tick to keep the
            // loop responsive in tests.
            let next_resync_in = self
                .inner
                .cfg
                .frequency
                .resync_period
                .saturating_sub(last_sync.elapsed());
            let until_deadline = deadline.saturating_sub(started.elapsed());
            let tick = next_resync_in.min(until_deadline).min(Duration::from_millis(20));

            let recv = async {
                let mut rx = self.inner.rx.lock().await;
                rx.recv().await
            };
            let event_or_timeout = tokio::time::timeout(tick, recv).await;
            match event_or_timeout {
                Ok(Some(ev)) => {
                    self.apply_event(ev);
                    // Best-effort debounce: drain anything else available
                    // immediately so we coalesce a burst.
                    let mut rx = self.inner.rx.lock().await;
                    while let Ok(more) = rx.try_recv() {
                        self.apply_event(more);
                    }
                }
                Ok(None) => {
                    // Source closed — do final sync (if dirty) and keep
                    // looping so periodic resyncs can still fire until
                    // deadline elapses.
                }
                Err(_elapsed) => {
                    // Timer fired — drop through to resync check.
                }
            }

            let dirty = self.inner.services.is_dirty() || self.inner.endpoints.is_dirty();
            let resync_due = last_sync.elapsed() >= self.inner.cfg.frequency.resync_period;
            if dirty || resync_due {
                self.sync_once().await?;
                last_sync = Instant::now();
            }
        }
    }
}
