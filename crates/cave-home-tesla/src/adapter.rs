// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The [`EnergyProvider`] trait and its Tesla implementation.
//!
//! [`TeslaProvider`] ties the [`FleetClient`](crate::fleet_api::client::FleetClient)
//! together with a [`StateCache`]: every read is parsed into the
//! [`crate::models`] domain model, cached, and — when the API is briefly
//! unreachable — served from that cache for up to a TTL (5 minutes by default)
//! so the household-facing surfaces keep working through a cloud blip.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::error::{Result, TeslaError};
use crate::fleet_api::client::{send_with_retry, Backoff, Clock, FleetClient, HttpTransport};
use crate::fleet_api::endpoints::{EnergyEndpoint, HistoryKind, HistoryPeriod};
use crate::fleet_api::types::{Envelope, HistorySeries, LiveStatus, SiteInfo};
use crate::models::{BatteryData, DateRange, HistoryData, OpMode, PowerFlowData, SiteStatus};

/// The clean energy surface cave-home consumes, independent of Tesla.
///
/// Implementors translate the household's intent into whatever protocol their
/// hardware speaks; cave-home (Portal, CLI, voice, automations) only ever sees
/// this trait and the [`crate::models`] domain types.
#[async_trait]
pub trait EnergyProvider {
    /// The overall site status (grid connection, mode, storm, reserve).
    async fn get_status(&self) -> Result<SiteStatus>;
    /// The instantaneous power flow + state of charge.
    async fn get_power_flow(&self) -> Result<PowerFlowData>;
    /// Set the backup reserve, percent (0..=100).
    async fn set_backup_reserve(&self, percent: u8) -> Result<()>;
    /// Set the operation mode.
    async fn set_operation_mode(&self, mode: OpMode) -> Result<()>;
    /// Fetch the historical power series over `range`.
    async fn get_history(&self, range: DateRange) -> Result<HistoryData>;
}

/// A short-lived cache of the last good readings, served while the API is
/// briefly unreachable.
#[derive(Debug)]
pub struct StateCache {
    ttl_ms: u64,
    flow: Mutex<Option<(u64, PowerFlowData)>>,
    status: Mutex<Option<(u64, SiteStatus)>>,
}

impl StateCache {
    /// A cache that serves entries for up to `ttl_ms` milliseconds.
    #[must_use]
    pub const fn new(ttl_ms: u64) -> Self {
        Self {
            ttl_ms,
            flow: Mutex::new(None),
            status: Mutex::new(None),
        }
    }

    fn put_flow(&self, now_ms: u64, flow: PowerFlowData) {
        *self.flow.lock() = Some((now_ms, flow));
    }

    fn get_flow(&self, now_ms: u64) -> Option<PowerFlowData> {
        self.flow
            .lock()
            .filter(|(at, _)| now_ms.saturating_sub(*at) <= self.ttl_ms)
            .map(|(_, f)| f)
    }

    fn put_status(&self, now_ms: u64, status: SiteStatus) {
        *self.status.lock() = Some((now_ms, status));
    }

    fn get_status(&self, now_ms: u64) -> Option<SiteStatus> {
        self.status
            .lock()
            .clone()
            .filter(|(at, _)| now_ms.saturating_sub(*at) <= self.ttl_ms)
            .map(|(_, s)| s)
    }
}

/// The Tesla implementation of [`EnergyProvider`] over a [`FleetClient`].
pub struct TeslaProvider<T: HttpTransport> {
    client: FleetClient<T>,
    site_id: u64,
    clock: Arc<dyn Clock>,
    cache: StateCache,
    backoff: Backoff,
    max_retries: u32,
}

impl<T: HttpTransport> TeslaProvider<T> {
    /// Build a provider for `site_id`, driven by `clock`, with a 5-minute
    /// resilience cache and a sane retry policy.
    #[must_use]
    pub fn new(client: FleetClient<T>, site_id: u64, clock: Arc<dyn Clock>) -> Self {
        Self {
            client,
            site_id,
            clock,
            cache: StateCache::new(300_000),
            backoff: Backoff::new(1_000, 60_000),
            max_retries: 3,
        }
    }

    /// Override the retry count (0 = no retries).
    #[must_use]
    pub const fn with_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Override the resilience-cache TTL, in seconds.
    #[must_use]
    pub fn with_cache_ttl_secs(mut self, secs: u64) -> Self {
        self.cache = StateCache::new(secs.saturating_mul(1_000));
        self
    }

    /// Borrow the underlying client (used by tests and metrics wiring).
    #[must_use]
    pub const fn client(&self) -> &FleetClient<T> {
        &self.client
    }

    /// The latest battery view (derived from the same `live_status` call).
    ///
    /// # Errors
    /// Propagates [`get_power_flow`](Self::get_power_flow)'s errors.
    pub async fn get_battery(&self) -> Result<BatteryData> {
        let live = self.fetch_live().await?;
        Ok(BatteryData::from(&live))
    }

    async fn fetch_live(&self) -> Result<LiveStatus> {
        let resp = send_with_retry(
            &self.client,
            &EnergyEndpoint::LiveStatus(self.site_id),
            self.clock.as_ref(),
            self.backoff,
            self.max_retries,
        )
        .await?;
        let env: Envelope<LiveStatus> = serde_json::from_str(&resp.body)?;
        Ok(env.response)
    }

    async fn fetch_info(&self) -> Result<SiteInfo> {
        let resp = send_with_retry(
            &self.client,
            &EnergyEndpoint::SiteInfo(self.site_id),
            self.clock.as_ref(),
            self.backoff,
            self.max_retries,
        )
        .await?;
        let env: Envelope<SiteInfo> = serde_json::from_str(&resp.body)?;
        Ok(env.response)
    }

    /// The calendar-history period that best covers `range`.
    const fn period_for(range: DateRange) -> HistoryPeriod {
        let hours = range.duration_secs() / 3_600;
        match hours {
            0..=26 => HistoryPeriod::Day,
            27..=192 => HistoryPeriod::Week,
            193..=768 => HistoryPeriod::Month,
            _ => HistoryPeriod::Year,
        }
    }
}

#[async_trait]
impl<T: HttpTransport> EnergyProvider for TeslaProvider<T> {
    async fn get_power_flow(&self) -> Result<PowerFlowData> {
        let now = self.clock.now_millis();
        match self.fetch_live().await {
            Ok(live) => {
                let flow = PowerFlowData::from(&live);
                self.cache.put_flow(now, flow);
                Ok(flow)
            }
            Err(e) => self.cache.get_flow(now).ok_or(e),
        }
    }

    async fn get_status(&self) -> Result<SiteStatus> {
        let now = self.clock.now_millis();
        let live = self.fetch_live().await;
        let info = self.fetch_info().await;
        match (live, info) {
            (Ok(live), Ok(info)) => {
                let status = SiteStatus::from_parts(&live, Some(&info));
                self.cache.put_status(now, status.clone());
                Ok(status)
            }
            // Live alone still yields a useful status (mode/reserve absent).
            (Ok(live), Err(_)) => {
                let status = SiteStatus::from_parts(&live, None);
                self.cache.put_status(now, status.clone());
                Ok(status)
            }
            (Err(e), _) => self.cache.get_status(now).ok_or(e),
        }
    }

    async fn set_backup_reserve(&self, percent: u8) -> Result<()> {
        if percent > 100 {
            return Err(TeslaError::Validation(format!(
                "backup reserve must be 0..=100, got {percent}"
            )));
        }
        send_with_retry(
            &self.client,
            &EnergyEndpoint::SetBackupReserve {
                site_id: self.site_id,
                percent,
            },
            self.clock.as_ref(),
            self.backoff,
            self.max_retries,
        )
        .await?;
        Ok(())
    }

    async fn set_operation_mode(&self, mode: OpMode) -> Result<()> {
        send_with_retry(
            &self.client,
            &EnergyEndpoint::SetOperationMode {
                site_id: self.site_id,
                mode: mode.wire(),
            },
            self.clock.as_ref(),
            self.backoff,
            self.max_retries,
        )
        .await?;
        Ok(())
    }

    async fn get_history(&self, range: DateRange) -> Result<HistoryData> {
        let resp = send_with_retry(
            &self.client,
            &EnergyEndpoint::History {
                site_id: self.site_id,
                kind: HistoryKind::Power,
                period: Self::period_for(range),
            },
            self.clock.as_ref(),
            self.backoff,
            self.max_retries,
        )
        .await?;
        let env: Envelope<HistorySeries> = serde_json::from_str(&resp.body)?;
        Ok(HistoryData::from(&env.response))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::fleet_api::client::{FleetClient, ManualClock, MockTransport};
    use crate::fleet_api::Region;
    use crate::models::OpMode;

    const LIVE: &str = r#"{"response":{"solar_power":3000,"battery_power":-1000,"load_power":2000,
        "grid_power":0,"percentage_charged":80,"energy_left":10800,"total_pack_energy":13500,
        "grid_status":"Active","island_status":"on_grid","storm_mode_active":false}}"#;
    const INFO: &str = r#"{"response":{"id":"STE1","site_name":"Cave","backup_reserve_percent":20,
        "default_real_mode":"self_consumption","battery_count":1,"nameplate_power":5000,
        "nameplate_energy":13500}}"#;

    fn provider(t: MockTransport, clock: Arc<ManualClock>) -> TeslaProvider<MockTransport> {
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        TeslaProvider::new(client, 1, clock).with_retries(0)
    }

    #[tokio::test]
    async fn get_power_flow_parses_and_caches() {
        let clock = Arc::new(ManualClock::new(0));
        let p = provider(MockTransport::new().route("/live_status", 200, LIVE), clock);
        let f = p.get_power_flow().await.unwrap();
        assert!((f.pv_watts - 3000.0).abs() < f64::EPSILON);
        assert!(f.battery_charging());
        assert!((f.soc_percent - 80.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn get_power_flow_served_from_cache_on_failure() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/live_status", 200, LIVE);
        let p = provider(t, clock.clone());
        // Prime the cache.
        let _ = p.get_power_flow().await.unwrap();
        // Now the API goes dark; advance past the rate-limit but within TTL.
        p.client().transport().set_failure(Some("network down".into()));
        clock.advance(60_000);
        let f = p.get_power_flow().await.unwrap();
        assert!((f.pv_watts - 3000.0).abs() < f64::EPSILON, "served stale snapshot");
    }

    #[tokio::test]
    async fn get_power_flow_errors_when_failing_with_no_cache() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new();
        t.set_failure(Some("network down".into()));
        let p = provider(t, clock);
        assert!(p.get_power_flow().await.is_err());
    }

    #[tokio::test]
    async fn get_power_flow_errors_when_cache_is_stale() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/live_status", 200, LIVE);
        let p = provider(t, clock.clone()).with_cache_ttl_secs(300);
        let _ = p.get_power_flow().await.unwrap();
        p.client().transport().set_failure(Some("down".into()));
        // Past both the rate limit and the 5-minute TTL.
        clock.advance(301_000);
        assert!(p.get_power_flow().await.is_err());
    }

    #[tokio::test]
    async fn get_status_merges_live_and_info() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new()
            .route("/live_status", 200, LIVE)
            .route("/site_info", 200, INFO);
        let p = provider(t, clock);
        let s = p.get_status().await.unwrap();
        assert_eq!(s.name.as_deref(), Some("Cave"));
        assert!(s.grid_connected);
        assert_eq!(s.op_mode, Some(OpMode::SelfConsumption));
        assert_eq!(s.backup_reserve_percent, Some(20));
    }

    #[tokio::test]
    async fn set_backup_reserve_rejects_over_100() {
        let clock = Arc::new(ManualClock::new(0));
        let p = provider(MockTransport::new(), clock);
        let err = p.set_backup_reserve(150).await.unwrap_err();
        assert!(matches!(err, TeslaError::Validation(_)));
        // No request should have been attempted.
        assert_eq!(p.client().transport().request_count(), 0);
    }

    #[tokio::test]
    async fn set_backup_reserve_posts_command() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/backup", 200, r#"{"response":{"result":true}}"#);
        let p = provider(t, clock);
        p.set_backup_reserve(25).await.unwrap();
        let reqs = p.client().transport().requests.lock();
        assert!(reqs[0].body.as_deref().unwrap().contains("\"backup_reserve_percent\":25"));
    }

    #[tokio::test]
    async fn set_operation_mode_posts_wire_value() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/operation", 200, r#"{"response":{"result":true}}"#);
        let p = provider(t, clock);
        p.set_operation_mode(OpMode::Backup).await.unwrap();
        let reqs = p.client().transport().requests.lock();
        assert!(reqs[0].body.as_deref().unwrap().contains("\"default_real_mode\":\"backup\""));
    }

    #[tokio::test]
    async fn get_history_maps_series() {
        let clock = Arc::new(ManualClock::new(0));
        let hist = r#"{"response":{"period":"day","time_series":[
            {"timestamp":"t0","solar_power":0,"battery_power":0,"grid_power":0},
            {"timestamp":"t1","solar_power":4200,"battery_power":0,"grid_power":0}]}}"#;
        let t = MockTransport::new().route("/calendar_history", 200, hist);
        let p = provider(t, clock);
        let range = DateRange::last_hours(1_000_000, 24);
        let h = p.get_history(range).await.unwrap();
        assert_eq!(h.samples.len(), 2);
        assert!((h.peak_pv_watts() - 4200.0).abs() < f64::EPSILON);
    }
}
