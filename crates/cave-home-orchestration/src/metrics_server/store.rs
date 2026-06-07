//! In-memory ring-buffer point storage and the cumulative-counter → CPU-rate
//! computation.
//!
//! The `pkg/storage` slice: `point.go`'s `resourceUsage` plus the node / pod stores.
//!
//! metrics-server keeps the most recent samples per object in memory and derives
//! a CPU usage *rate* from the cumulative `usageCoreNanoSeconds` counter between
//! two points:
//!
//! ```text
//! nanocores = (last.cumulative_cpu_nanos − prev.cumulative_cpu_nanos) · 1e9
//!             ────────────────────────────────────────────────────────────
//!                     (last.timestamp_nanos − prev.timestamp_nanos)
//! ```
//!
//! Memory is a gauge, taken straight from the latest point. A counter that went
//! **backwards** (container restart / cgroup reset) or a **non-increasing
//! timestamp** (zero window) yields an error instead of a bogus rate, and fewer
//! than two points means there is no rate yet — exactly the guards upstream
//! `resourceUsage` applies.
//!
//! Storage is a [`Storage`] of per-node [`PointRing`]s plus per-pod,
//! per-container rings, keyed deterministically (`BTreeMap`) so enumeration and
//! the API output are stable. The ring capacity defaults to two (upstream keeps
//! exactly prev + last); a larger history can be requested without changing the
//! rate, which always uses the two most recent points.

use std::collections::BTreeMap;

use super::quantity::{Quantity, ResourceList};
use super::summary::MetricsPoint;

/// One nanocore = `1e9` per core; the scale that turns CPU-nanoseconds-per-
/// wall-nanosecond into nanocores.
const NANO: u128 = 1_000_000_000;

/// Why a usage rate could not be derived from a pair of points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateError {
    /// Fewer than two points are stored — no window to rate over yet.
    InsufficientData,
    /// The CPU counter decreased between the two points (a restart / reset).
    CounterReset,
    /// The later point's timestamp is not strictly after the earlier one's.
    NonMonotonicTime,
}

/// A derived usage reading: the `{cpu, memory}` list plus the window it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// The end of the window — the latest point's timestamp (nanoseconds).
    pub timestamp_nanos: u64,
    /// The window width the CPU rate was averaged over (nanoseconds).
    pub window_nanos: u64,
    /// The derived usage: CPU rate (nanocores) + memory gauge (bytes).
    pub usage: ResourceList,
}

impl Usage {
    /// Derive a usage reading from an earlier and a later point.
    ///
    /// # Errors
    /// - [`RateError::NonMonotonicTime`] if `last` is not strictly after `prev`.
    /// - [`RateError::CounterReset`] if the CPU counter decreased.
    pub fn between(prev: MetricsPoint, last: MetricsPoint) -> Result<Self, RateError> {
        if last.timestamp_nanos <= prev.timestamp_nanos {
            return Err(RateError::NonMonotonicTime);
        }
        if last.cumulative_cpu_nanos < prev.cumulative_cpu_nanos {
            return Err(RateError::CounterReset);
        }
        let window = last.timestamp_nanos - prev.timestamp_nanos;
        let delta_cpu = u128::from(last.cumulative_cpu_nanos - prev.cumulative_cpu_nanos);
        // nanocores = Δcpu_ns · 1e9 / window_ns; u128 keeps Δcpu·1e9 (≤ ~1e27
        // for realistic counters) from overflowing.
        let nanocores = (delta_cpu * NANO) / u128::from(window);
        let cpu = Quantity::from_cpu_nanocores(u64::try_from(nanocores).unwrap_or(u64::MAX));
        Ok(Self {
            timestamp_nanos: last.timestamp_nanos,
            window_nanos: window,
            usage: ResourceList::new(cpu, Quantity::from_bytes(last.working_set_bytes)),
        })
    }
}

/// A fixed-capacity ring of recent [`MetricsPoint`]s for one object.
///
/// Pushing past the capacity evicts the oldest. The rate uses the two most
/// recent points; a larger capacity simply retains more history.
#[derive(Debug, Clone)]
pub struct PointRing {
    points: Vec<MetricsPoint>,
    capacity: usize,
}

impl PointRing {
    /// A ring holding at most `capacity` points (clamped to a minimum of two,
    /// the count a rate needs).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            points: Vec::new(),
            capacity: capacity.max(2),
        }
    }

    /// Append a point, evicting the oldest if at capacity.
    pub fn push(&mut self, point: MetricsPoint) {
        if self.points.len() == self.capacity {
            self.points.remove(0);
        }
        self.points.push(point);
    }

    /// How many points are currently retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether the ring holds no points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Derive the usage from the two most recent points.
    ///
    /// # Errors
    /// [`RateError::InsufficientData`] with fewer than two points; otherwise the
    /// errors of [`Usage::between`].
    pub fn usage(&self) -> Result<Usage, RateError> {
        let n = self.points.len();
        if n < 2 {
            return Err(RateError::InsufficientData);
        }
        Usage::between(self.points[n - 2], self.points[n - 1])
    }
}

impl Default for PointRing {
    fn default() -> Self {
        Self::with_capacity(2)
    }
}

/// A pod's identity as a storage key: `(namespace, name)`.
type PodKey = (String, String);

/// The in-memory metric store: per-node rings + per-pod, per-container rings.
#[derive(Debug, Clone, Default)]
pub struct Storage {
    capacity: usize,
    nodes: BTreeMap<String, PointRing>,
    pods: BTreeMap<PodKey, BTreeMap<String, PointRing>>,
}

impl Storage {
    /// A store whose rings keep the upstream default of two points each.
    #[must_use]
    pub fn new() -> Self {
        Self::with_history(2)
    }

    /// A store whose rings keep `capacity` points each (min two).
    #[must_use]
    pub fn with_history(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(2),
            nodes: BTreeMap::new(),
            pods: BTreeMap::new(),
        }
    }

    /// Record a node sample.
    pub fn store_node(&mut self, node: &str, point: MetricsPoint) {
        let cap = self.capacity;
        self.nodes
            .entry(node.to_string())
            .or_insert_with(|| PointRing::with_capacity(cap))
            .push(point);
    }

    /// Record a container sample for a pod identified by `namespace`/`pod`.
    pub fn store_container(
        &mut self,
        namespace: &str,
        pod: &str,
        container: &str,
        point: MetricsPoint,
    ) {
        let cap = self.capacity;
        self.pods
            .entry((namespace.to_string(), pod.to_string()))
            .or_default()
            .entry(container.to_string())
            .or_insert_with(|| PointRing::with_capacity(cap))
            .push(point);
    }

    /// The derived usage for a node: `None` if the node is unknown, else the
    /// rate result (which may itself be an error if only one point is stored).
    #[must_use]
    pub fn node_usage(&self, node: &str) -> Option<Result<Usage, RateError>> {
        self.nodes.get(node).map(PointRing::usage)
    }

    /// The per-container usages of a pod, sorted by container name. Containers
    /// whose rate cannot be derived yet are omitted.
    #[must_use]
    pub fn pod_container_usages(&self, namespace: &str, pod: &str) -> Vec<(String, Usage)> {
        let key = (namespace.to_string(), pod.to_string());
        let Some(containers) = self.pods.get(&key) else {
            return Vec::new();
        };
        containers
            .iter()
            .filter_map(|(name, ring)| ring.usage().ok().map(|u| (name.clone(), u)))
            .collect()
    }

    /// Every node name with a ring, sorted.
    #[must_use]
    pub fn node_names(&self) -> Vec<String> {
        self.nodes.keys().cloned().collect()
    }

    /// Every `(namespace, pod)` key with a ring, sorted.
    #[must_use]
    pub fn pod_keys(&self) -> Vec<PodKey> {
        self.pods.keys().cloned().collect()
    }

    /// Number of nodes tracked.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of pods tracked.
    #[must_use]
    pub fn pod_count(&self) -> usize {
        self.pods.len()
    }

    /// Total points retained across every ring — the memory-footprint figure the
    /// observability track exports.
    #[must_use]
    pub fn points_stored(&self) -> usize {
        let node_pts: usize = self.nodes.values().map(PointRing::len).sum();
        let pod_pts: usize = self
            .pods
            .values()
            .flat_map(|cs| cs.values())
            .map(PointRing::len)
            .sum();
        node_pts + pod_pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: u64, cpu: u64, mem: u64) -> MetricsPoint {
        MetricsPoint {
            timestamp_nanos: ts,
            cumulative_cpu_nanos: cpu,
            working_set_bytes: mem,
        }
    }

    #[test]
    fn ring_capacity_floor_is_two() {
        let ring = PointRing::with_capacity(0);
        assert_eq!(ring.capacity, 2);
        assert!(ring.is_empty());
    }

    #[test]
    fn ring_history_can_exceed_two_but_rate_uses_latest_pair() {
        let mut ring = PointRing::with_capacity(5);
        ring.push(point(0, 0, 1));
        ring.push(point(1_000_000_000, 100_000_000, 2));
        ring.push(point(2_000_000_000, 300_000_000, 3));
        assert_eq!(ring.len(), 3);
        // Latest pair: Δcpu 200_000_000 over 1s → 200m.
        assert_eq!(ring.usage().expect("ok").usage.cpu.to_cpu_string(), "200m");
    }

    #[test]
    fn unknown_node_usage_is_none() {
        let store = Storage::new();
        assert!(store.node_usage("nope").is_none());
        assert!(store.pod_container_usages("ns", "nope").is_empty());
    }

    #[test]
    fn enumeration_is_sorted() {
        let mut store = Storage::new();
        store.store_node("b", point(0, 0, 1));
        store.store_node("a", point(0, 0, 1));
        assert_eq!(store.node_names(), vec!["a", "b"]);
    }
}
