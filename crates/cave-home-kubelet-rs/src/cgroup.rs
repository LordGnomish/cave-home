// SPDX-License-Identifier: Apache-2.0
//! cgroup **v2** resource conversion + the `QoS` `cgroupfs` hierarchy.
//!
//! Behavioural reimplementation of the documented kubelet container-manager
//! decisions that translate a pod/container's [`ResourceRequirements`] into the
//! cgroup v2 unified-hierarchy control files, and that lay out the
//! `/kubepods` `QoS` cgroup tree.
//!
//! This kubelet is **cgroup v2 only** (Charter §8 no-backcompat): the cgroup v1
//! `cpu.shares` / `cpu.cfs_quota_us` / `memory.limit_in_bytes` files are not
//! emitted — only the v2 `cpu.weight` / `cpu.max` / `memory.max` values.
//!
//! Spec sources:
//!   * `pkg/kubelet/cm/helpers_linux.go` — `MilliCPUToShares` (`milli*1024/1000`,
//!     floored to `MinShares == 2`) and `MilliCPUToQuota`
//!     (`milli*period/1000`, floored to `MinQuotaPeriod == 1000µs`).
//!   * the libcontainer / systemd cgroup-v2 weight conversion
//!     `1 + ((shares - 2) * 9999) / 262142`, mapping shares `[2, 262144]` onto
//!     `cpu.weight` `[1, 10000]`.
//!   * `pkg/kubelet/cm/cgroup_manager_linux.go` — the `cpu.max` (`"<quota>
//!     <period>"` or `"max <period>"`) and `memory.max` (`"<bytes>"` or
//!     `"max"`) unified-hierarchy values.
//!   * `pkg/kubelet/cm/qos_container_manager_linux.go` +
//!     `pod_container_manager_linux.go` — the cgroupfs hierarchy
//!     `/kubepods{,/burstable,/besteffort}/pod<uid>`; Guaranteed pods sit
//!     directly under the root, Burstable/`BestEffort` under their `QoS` subtree.
//!
//! Pure, `std`-only: this computes the *values* and *paths*; writing them to
//! `sysfs` (the `cgroup.subtree_control` delegation, the `mkdir`/`write`
//! syscalls) is the deferred runtime layer (see `parity.manifest.toml`).

use crate::eviction::QosClass;
use crate::resources::ResourceRequirements;

/// The default CFS period, in microseconds (`100ms`).
pub const DEFAULT_CPU_PERIOD_US: u64 = 100_000;

/// The minimum cgroup CPU shares (`MinShares`); a container with no CPU request
/// still gets this floor.
pub const MIN_SHARES: u64 = 2;

/// CPU shares granted per whole core (`SharesPerCPU`).
pub const SHARES_PER_CPU: u64 = 1024;

/// Milli-cores per whole core (`MilliCPUToCPU`).
pub const MILLI_PER_CPU: u64 = 1000;

/// The minimum CFS quota, in microseconds (`MinQuotaPeriod`).
pub const MIN_QUOTA_US: u64 = 1000;

/// The maximum cgroup v2 `cpu.weight`.
pub const MAX_CPU_WEIGHT: u64 = 10_000;

/// The cgroup v1 share value that maps to the maximum v2 weight.
const SHARES_AT_MAX_WEIGHT: u64 = 262_144;

/// Converts a CPU request in milli-cores to cgroup CPU shares.
///
/// `MilliCPUToShares`: `milli * 1024 / 1000`, floored to [`MIN_SHARES`]. A zero
/// request yields the floor.
#[must_use]
pub const fn milli_cpu_to_shares(milli: u64) -> u64 {
    let shares = (milli * SHARES_PER_CPU) / MILLI_PER_CPU;
    if shares < MIN_SHARES {
        MIN_SHARES
    } else {
        shares
    }
}

/// Converts cgroup CPU shares to a cgroup v2 `cpu.weight`.
///
/// `1 + ((shares - 2) * 9999) / 262142`, clamped to `[1, 10000]`.
#[must_use]
pub const fn shares_to_cpu_weight(shares: u64) -> u64 {
    if shares <= MIN_SHARES {
        return 1;
    }
    if shares >= SHARES_AT_MAX_WEIGHT {
        return MAX_CPU_WEIGHT;
    }
    1 + ((shares - MIN_SHARES) * 9999) / (SHARES_AT_MAX_WEIGHT - MIN_SHARES)
}

/// Converts a CPU request in milli-cores straight to a cgroup v2 `cpu.weight`
/// (composes [`milli_cpu_to_shares`] then [`shares_to_cpu_weight`]).
#[must_use]
pub const fn milli_cpu_to_cpu_weight(milli: u64) -> u64 {
    shares_to_cpu_weight(milli_cpu_to_shares(milli))
}

/// Converts a CPU limit in milli-cores to a CFS quota in microseconds.
///
/// `MilliCPUToQuota`: `milli * period / 1000`, floored to [`MIN_QUOTA_US`] for
/// any non-zero limit.
#[must_use]
pub const fn milli_cpu_to_quota_us(milli: u64, period_us: u64) -> u64 {
    if milli == 0 {
        return 0;
    }
    let quota = (milli * period_us) / MILLI_PER_CPU;
    if quota < MIN_QUOTA_US {
        MIN_QUOTA_US
    } else {
        quota
    }
}

/// Renders the cgroup v2 `cpu.max` value for an optional CPU limit.
///
/// `"<quota> <period>"` when limited, `"max <period>"` when unlimited.
#[must_use]
pub fn cpu_max(limit_milli: Option<u64>, period_us: u64) -> String {
    limit_milli.map_or_else(
        || format!("max {period_us}"),
        |milli| format!("{} {}", milli_cpu_to_quota_us(milli, period_us), period_us),
    )
}

/// Renders the cgroup v2 `memory.max` value for an optional memory limit.
///
/// The byte count when limited, the literal `"max"` when unlimited.
#[must_use]
pub fn memory_max(limit_bytes: Option<u64>) -> String {
    limit_bytes.map_or_else(|| "max".to_string(), |bytes| bytes.to_string())
}

/// The cgroup v2 control-file values for one container/pod, derived from its
/// [`ResourceRequirements`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CgroupV2Resources {
    /// `cpu.weight` (from the CPU **request**).
    pub cpu_weight: u64,
    /// `cpu.max` (from the CPU **limit**).
    pub cpu_max: String,
    /// `memory.max` (from the memory **limit**).
    pub memory_max: String,
}

impl CgroupV2Resources {
    /// Derives the cgroup v2 values from `req` using the [`DEFAULT_CPU_PERIOD_US`]
    /// CFS period. The CPU weight follows the *request*; the `cpu.max` /
    /// `memory.max` ceilings follow the *limits* (unbounded when unset).
    #[must_use]
    pub fn from_requirements(req: &ResourceRequirements) -> Self {
        Self {
            cpu_weight: milli_cpu_to_cpu_weight(req.cpu_request_milli.unwrap_or(0)),
            cpu_max: cpu_max(req.cpu_limit_milli, DEFAULT_CPU_PERIOD_US),
            memory_max: memory_max(req.memory_limit_bytes),
        }
    }
}

/// The default cgroup root the kubelet manages (`--cgroup-root` default).
pub const DEFAULT_CGROUP_ROOT: &str = "/kubepods";

/// The `QoS` `cgroupfs` hierarchy: the
/// `/kubepods{,/burstable,/besteffort}/pod<uid>` tree under a configurable root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CgroupHierarchy {
    root: String,
}

impl Default for CgroupHierarchy {
    fn default() -> Self {
        Self::with_root(DEFAULT_CGROUP_ROOT)
    }
}

impl CgroupHierarchy {
    /// A hierarchy rooted at `root` (a trailing slash is normalised away).
    #[must_use]
    pub fn with_root(root: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            root: root.trim_end_matches('/').to_string(),
        }
    }

    /// The cgroup root.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// The `QoS` sub-path under the root. Guaranteed pods live directly under the
    /// root (empty sub-path); Burstable / `BestEffort` each get a subtree.
    #[must_use]
    pub const fn qos_subpath(qos: QosClass) -> &'static str {
        match qos {
            QosClass::Guaranteed => "",
            QosClass::Burstable => "/burstable",
            QosClass::BestEffort => "/besteffort",
        }
    }

    /// The cgroup path for a pod of `QoS` class `qos` and id `uid`:
    /// `<root>[/<qos>]/pod<uid>`.
    #[must_use]
    pub fn pod_path(&self, qos: QosClass, uid: &str) -> String {
        format!("{}{}/pod{}", self.root, Self::qos_subpath(qos), uid)
    }

    /// The cgroup path for a container nested under its pod cgroup:
    /// `<pod-path>/<container_id>`.
    #[must_use]
    pub fn container_path(&self, qos: QosClass, uid: &str, container_id: &str) -> String {
        format!("{}/{}", self.pod_path(qos, uid), container_id)
    }
}
