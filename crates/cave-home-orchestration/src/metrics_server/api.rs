//! The `metrics.k8s.io/v1beta1` resource-metrics objects and the aggregated
//! `APIService` registration.
//!
//! The `pkg/api` surface plus the aggregation-layer wiring.
//!
//! metrics-server serves `NodeMetrics` and `PodMetrics` through the Kubernetes
//! **aggregation layer**: it registers an `APIService` (`v1beta1.metrics.k8s.io`)
//! that points the apiserver at the metrics service, then answers GET / LIST
//! with objects built from its [`Storage`]. A [`NodeMetrics`] carries one usage
//! list; a [`PodMetrics`] carries per-container usage and a [`PodMetrics::total`]
//! that sums them (what `kubectl top pods` shows).
//!
//! This module builds those objects from [`Usage`] readings and describes the
//! [`ApiService`] to register. The actual aggregation-layer serving (HTTP, the
//! apiserver proxy, TLS) is runtime-bound (ADR-004 phase-1b); the object
//! shaping and the registration descriptor are the decisions modelled here.

use super::quantity::ResourceList;
use super::store::{Storage, Usage};

/// The API group metrics-server serves.
pub const GROUP: &str = "metrics.k8s.io";

/// The API version metrics-server serves.
pub const VERSION: &str = "v1beta1";

/// `apiVersion` string (`group/version`) stamped on every object.
const API_VERSION: &str = "metrics.k8s.io/v1beta1";

/// A node's resource usage — `metrics.k8s.io/v1beta1` `NodeMetrics`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeMetrics {
    /// The node name (the object's `metadata.name`).
    pub name: String,
    /// The window end — when the underlying sample was taken (nanoseconds).
    pub timestamp_nanos: u64,
    /// The averaging window the CPU rate covers (nanoseconds).
    pub window_nanos: u64,
    /// CPU + memory usage.
    pub usage: ResourceList,
}

impl NodeMetrics {
    /// The object kind (`typeMeta.kind`).
    pub const KIND: &'static str = "NodeMetrics";

    /// Build a `NodeMetrics` for `name` from a derived [`Usage`].
    #[must_use]
    pub fn from_usage(name: &str, usage: &Usage) -> Self {
        Self {
            name: name.to_string(),
            timestamp_nanos: usage.timestamp_nanos,
            window_nanos: usage.window_nanos,
            usage: usage.usage,
        }
    }

    /// The `apiVersion` string.
    #[must_use]
    pub const fn api_version(&self) -> &'static str {
        API_VERSION
    }

    /// List `NodeMetrics` for every node in `storage` whose rate is derivable;
    /// nodes with too few samples (or a counter reset) are skipped, exactly as
    /// the aggregation layer returns only nodes it has a reading for.
    #[must_use]
    pub fn list_from_storage(storage: &Storage) -> Vec<Self> {
        storage
            .node_names()
            .into_iter()
            .filter_map(|name| {
                storage
                    .node_usage(&name)
                    .and_then(Result::ok)
                    .map(|u| Self::from_usage(&name, &u))
            })
            .collect()
    }
}

/// One container's usage inside a [`PodMetrics`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerMetrics {
    /// The container name.
    pub name: String,
    /// CPU + memory usage.
    pub usage: ResourceList,
}

impl ContainerMetrics {
    /// Pair a container name with its usage.
    #[must_use]
    pub fn new(name: &str, usage: ResourceList) -> Self {
        Self {
            name: name.to_string(),
            usage,
        }
    }
}

/// A pod's per-container resource usage — `metrics.k8s.io/v1beta1` `PodMetrics`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodMetrics {
    /// The pod namespace.
    pub namespace: String,
    /// The pod name.
    pub name: String,
    /// The window end (the minimum container timestamp, nanoseconds).
    pub timestamp_nanos: u64,
    /// The averaging window (the minimum container window, nanoseconds).
    pub window_nanos: u64,
    /// Per-container usage, in container-name order.
    pub containers: Vec<ContainerMetrics>,
}

impl PodMetrics {
    /// The object kind (`typeMeta.kind`).
    pub const KIND: &'static str = "PodMetrics";

    /// Build a `PodMetrics` from a pod's per-container usages. The pod timestamp
    /// and window are the **minimum** across the containers (the conservative
    /// reading metrics-server reports), and an empty container set yields a
    /// zero-window pod.
    #[must_use]
    pub fn from_container_usages(
        namespace: &str,
        name: &str,
        containers: &[(String, Usage)],
    ) -> Self {
        let timestamp_nanos = containers
            .iter()
            .map(|(_, u)| u.timestamp_nanos)
            .min()
            .unwrap_or(0);
        let window_nanos = containers
            .iter()
            .map(|(_, u)| u.window_nanos)
            .min()
            .unwrap_or(0);
        let containers = containers
            .iter()
            .map(|(cn, u)| ContainerMetrics::new(cn, u.usage))
            .collect();
        Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            timestamp_nanos,
            window_nanos,
            containers,
        }
    }

    /// The pod total = the component-wise sum of its container usages (the
    /// figure `kubectl top pods` prints).
    #[must_use]
    pub fn total(&self) -> ResourceList {
        self.containers
            .iter()
            .fold(ResourceList::zero(), |acc, c| acc.saturating_add(c.usage))
    }

    /// The `apiVersion` string.
    #[must_use]
    pub const fn api_version(&self) -> &'static str {
        API_VERSION
    }

    /// List `PodMetrics` for every pod in `storage` that has at least one
    /// rateable container; pods with no derivable container rate are skipped.
    #[must_use]
    pub fn list_from_storage(storage: &Storage) -> Vec<Self> {
        storage
            .pod_keys()
            .into_iter()
            .filter_map(|(ns, name)| {
                let usages = storage.pod_container_usages(&ns, &name);
                if usages.is_empty() {
                    None
                } else {
                    Some(Self::from_container_usages(&ns, &name, &usages))
                }
            })
            .collect()
    }
}

/// The aggregated `APIService` metrics-server registers so the apiserver proxies
/// `metrics.k8s.io/v1beta1` to it (`apiregistration.k8s.io/v1` `APIService`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiService {
    /// The `APIService` object name — `<version>.<group>`.
    pub name: String,
    /// The served group.
    pub group: String,
    /// The served version.
    pub version: String,
    /// `groupPriorityMinimum` — the group's sort priority in discovery.
    pub group_priority_minimum: i32,
    /// `versionPriority` — the version's sort priority within the group.
    pub version_priority: i32,
    /// Whether the apiserver skips TLS verification of the backend (the
    /// in-cluster metrics service uses a self-signed serving cert).
    pub insecure_skip_tls_verify: bool,
}

impl ApiService {
    /// The canonical `v1beta1.metrics.k8s.io` registration metrics-server uses.
    #[must_use]
    pub fn metrics_v1beta1() -> Self {
        Self {
            name: format!("{VERSION}.{GROUP}"),
            group: GROUP.to_string(),
            version: VERSION.to_string(),
            // The priorities metrics-server's manifest sets.
            group_priority_minimum: 100,
            version_priority: 100,
            insecure_skip_tls_verify: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics_server::quantity::Quantity;

    fn usage(ts: u64, window: u64, cpu: u64, mem: u64) -> Usage {
        Usage {
            timestamp_nanos: ts,
            window_nanos: window,
            usage: ResourceList::new(Quantity::from_cpu_nanocores(cpu), Quantity::from_bytes(mem)),
        }
    }

    #[test]
    fn empty_pod_has_zero_total_and_window() {
        let pm = PodMetrics::from_container_usages("ns", "p", &[]);
        assert_eq!(pm.window_nanos, 0);
        assert_eq!(pm.total(), ResourceList::zero());
    }

    #[test]
    fn pod_total_folds_all_containers() {
        let cs = vec![
            ("a".to_string(), usage(1, 1, 10_000_000, 1)),
            ("b".to_string(), usage(2, 1, 20_000_000, 2)),
            ("c".to_string(), usage(3, 1, 30_000_000, 3)),
        ];
        let pm = PodMetrics::from_container_usages("ns", "p", &cs);
        assert_eq!(pm.total().cpu.to_cpu_string(), "60m");
        assert_eq!(pm.total().memory.raw(), 6);
        assert_eq!(pm.timestamp_nanos, 1);
    }

    #[test]
    fn apiservice_name_is_version_dot_group() {
        assert_eq!(ApiService::metrics_v1beta1().name, "v1beta1.metrics.k8s.io");
    }
}
