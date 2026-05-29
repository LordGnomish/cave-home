//! Connectivity summary — the at-a-glance health of the home network.
//!
//! This is the surface the Portal tile and the voice reply consume: how many
//! things are connected, how busy each Wi-Fi point is, how much data is
//! flowing, and the single question a household actually asks — *is the
//! internet up?*. All derived purely from the supplied model + samples; no I/O.

use std::collections::BTreeMap;

use crate::client::NetworkClient;
use crate::device::{DeviceKind, NetworkDevice};

/// One throughput sample: bytes seen since the previous sample, plus the tick
/// the sample was taken at. The caller supplies these; the crate aggregates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThroughputSample {
    pub tick: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl ThroughputSample {
    #[must_use]
    pub const fn new(tick: u64, rx_bytes: u64, tx_bytes: u64) -> Self {
        Self { tick, rx_bytes, tx_bytes }
    }

    /// Total bytes (received + transmitted) in this sample.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

/// Is the home online?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternetState {
    /// The gateway is online and its uplink is up.
    Up,
    /// The gateway is reachable but its internet uplink is down.
    NoUplink,
    /// No gateway is online (or none is present at all).
    GatewayDown,
}

impl InternetState {
    #[must_use]
    pub const fn is_up(self) -> bool {
        matches!(self, Self::Up)
    }
}

/// The aggregated connectivity summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectivitySummary {
    /// How many clients are connected (not counting blocked ones).
    pub connected_clients: usize,
    /// How many of those are guest clients.
    pub guest_clients: usize,
    /// Client count per access-point id, sorted by id for stable output.
    pub clients_per_ap: BTreeMap<String, usize>,
    /// Total bytes received across the supplied samples.
    pub total_rx_bytes: u64,
    /// Total bytes transmitted across the supplied samples.
    pub total_tx_bytes: u64,
    /// Whether the internet is up.
    pub internet: InternetState,
}

impl ConnectivitySummary {
    /// Total bytes both ways across the window.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_rx_bytes.saturating_add(self.total_tx_bytes)
    }
}

/// Derive whether the internet is up from the network's devices.
///
/// A [`DeviceKind::Gateway`] that is online **and** has an uplink recorded is
/// [`InternetState::Up`]; an online gateway with no uplink is
/// [`InternetState::NoUplink`]; anything else (offline or absent gateway) is
/// [`InternetState::GatewayDown`].
#[must_use]
pub fn internet_state(devices: &[NetworkDevice]) -> InternetState {
    let gateway = devices.iter().find(|d| d.kind() == DeviceKind::Gateway);
    match gateway {
        Some(g) if g.is_online() && g.uplink().is_some() => InternetState::Up,
        Some(g) if g.is_online() => InternetState::NoUplink,
        _ => InternetState::GatewayDown,
    }
}

/// Build the connectivity summary from devices, clients and throughput samples.
///
/// Blocked clients are excluded from the connected count and the per-AP counts
/// (cave-home has deliberately cut them off; they are not "connected" from the
/// household's point of view). Throughput is summed over every sample so the
/// caller can feed a rolling window.
#[must_use]
#[allow(clippy::similar_names)] // rx/tx byte totals mirror the struct fields.
pub fn summarize(
    devices: &[NetworkDevice],
    clients: &[NetworkClient],
    samples: &[ThroughputSample],
) -> ConnectivitySummary {
    let mut clients_per_ap: BTreeMap<String, usize> = BTreeMap::new();
    let mut connected_clients = 0;
    let mut guest_clients = 0;

    for c in clients {
        if c.is_blocked() {
            continue;
        }
        connected_clients += 1;
        if c.is_guest() {
            guest_clients += 1;
        }
        if let Some(ap) = c.connection().access_point() {
            *clients_per_ap.entry(ap.to_string()).or_insert(0) += 1;
        }
    }

    let mut total_rx_bytes: u64 = 0;
    let mut total_tx_bytes: u64 = 0;
    for s in samples {
        total_rx_bytes = total_rx_bytes.saturating_add(s.rx_bytes);
        total_tx_bytes = total_tx_bytes.saturating_add(s.tx_bytes);
    }

    ConnectivitySummary {
        connected_clients,
        guest_clients,
        clients_per_ap,
        total_rx_bytes,
        total_tx_bytes,
        internet: internet_state(devices),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gateway_up() -> NetworkDevice {
        NetworkDevice::new("gw", "Internet box", "00:gw", DeviceKind::Gateway)
            .uplinked_to("isp")
    }

    #[test]
    fn internet_up_when_gateway_online_with_uplink() {
        assert_eq!(internet_state(&[gateway_up()]), InternetState::Up);
        assert!(internet_state(&[gateway_up()]).is_up());
    }

    #[test]
    fn internet_no_uplink_when_gateway_online_without_uplink() {
        let gw = NetworkDevice::new("gw", "Internet box", "00:gw", DeviceKind::Gateway);
        assert_eq!(internet_state(&[gw]), InternetState::NoUplink);
    }

    #[test]
    fn internet_down_when_gateway_offline_or_absent() {
        let gw = NetworkDevice::new("gw", "Internet box", "00:gw", DeviceKind::Gateway)
            .uplinked_to("isp")
            .offline();
        assert_eq!(internet_state(&[gw]), InternetState::GatewayDown);
        assert_eq!(internet_state(&[]), InternetState::GatewayDown);
    }

    #[test]
    fn counts_exclude_blocked_clients() {
        let clients = [
            NetworkClient::new("a", "Phone").wireless("Home", "ap-1"),
            NetworkClient::new("b", "Tablet").wireless("Home", "ap-1").blocked(),
            NetworkClient::new("c", "Laptop").wireless("Home", "ap-2"),
        ];
        let s = summarize(&[gateway_up()], &clients, &[]);
        assert_eq!(s.connected_clients, 2);
        assert_eq!(s.clients_per_ap.get("ap-1"), Some(&1));
        assert_eq!(s.clients_per_ap.get("ap-2"), Some(&1));
    }

    #[test]
    fn per_ap_counts_group_clients() {
        let clients = [
            NetworkClient::new("a", "Phone").wireless("Home", "ap-1"),
            NetworkClient::new("b", "Tablet").wireless("Home", "ap-1"),
            NetworkClient::new("c", "TV").wired_to("sw-1"),
        ];
        let s = summarize(&[gateway_up()], &clients, &[]);
        assert_eq!(s.connected_clients, 3);
        // Wired client has no AP, so it is counted but not in any AP bucket.
        assert_eq!(s.clients_per_ap.get("ap-1"), Some(&2));
        assert_eq!(s.clients_per_ap.len(), 1);
    }

    #[test]
    fn guest_clients_counted_separately() {
        let clients = [
            NetworkClient::new("a", "Phone").wireless("Home", "ap-1"),
            NetworkClient::new("g", "Visitor").wireless("Guest", "ap-1").as_guest(),
        ];
        let s = summarize(&[gateway_up()], &clients, &[]);
        assert_eq!(s.connected_clients, 2);
        assert_eq!(s.guest_clients, 1);
    }

    #[test]
    fn throughput_aggregates_over_samples() {
        let samples = [
            ThroughputSample::new(0, 1000, 200),
            ThroughputSample::new(1, 500, 300),
            ThroughputSample::new(2, 0, 100),
        ];
        let s = summarize(&[gateway_up()], &[], &samples);
        assert_eq!(s.total_rx_bytes, 1500);
        assert_eq!(s.total_tx_bytes, 600);
        assert_eq!(s.total_bytes(), 2100);
    }

    #[test]
    fn throughput_saturates_does_not_overflow() {
        let samples = [
            ThroughputSample::new(0, u64::MAX, u64::MAX),
            ThroughputSample::new(1, 10, 10),
        ];
        let s = summarize(&[], &[], &samples);
        assert_eq!(s.total_rx_bytes, u64::MAX);
        assert_eq!(s.total_bytes(), u64::MAX);
    }

    #[test]
    fn empty_network_summary_is_zeroed_and_down() {
        let s = summarize(&[], &[], &[]);
        assert_eq!(s.connected_clients, 0);
        assert_eq!(s.guest_clients, 0);
        assert!(s.clients_per_ap.is_empty());
        assert_eq!(s.total_bytes(), 0);
        assert_eq!(s.internet, InternetState::GatewayDown);
    }
}
