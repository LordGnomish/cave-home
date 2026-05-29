// SPDX-License-Identifier: Apache-2.0
//! Minimal resource model shared by admission + eviction.
//!
//! A deliberately small stand-in for `k8s.io/apimachinery/pkg/api/resource`:
//! CPU is measured in **milli-cores** (`1000m == 1 core`) and memory in
//! **bytes**, both as `u64`. This is sufficient for the kubelet node-resource
//! accounting and eviction-ranking decision logic; the full `Quantity` parser
//! (SI/binary suffixes, sub-milli precision) is an `[[unmapped]]` follow-up.
//!
//! Pure, `std`-only.

/// A CPU+memory resource amount.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceList {
    /// CPU in milli-cores (`1000m == 1 core`).
    pub cpu_milli: u64,
    /// Memory in bytes.
    pub memory_bytes: u64,
}

impl ResourceList {
    /// Construct a resource list.
    #[must_use]
    pub const fn new(cpu_milli: u64, memory_bytes: u64) -> Self {
        Self {
            cpu_milli,
            memory_bytes,
        }
    }

    /// Saturating component-wise addition.
    #[must_use]
    pub const fn add(self, other: Self) -> Self {
        Self {
            cpu_milli: self.cpu_milli.saturating_add(other.cpu_milli),
            memory_bytes: self.memory_bytes.saturating_add(other.memory_bytes),
        }
    }

    /// True iff both components fit within `capacity` (`self <= capacity`).
    #[must_use]
    pub const fn fits_within(self, capacity: Self) -> bool {
        self.cpu_milli <= capacity.cpu_milli && self.memory_bytes <= capacity.memory_bytes
    }
}

/// A container's resource requests + limits (mirrors
/// `v1.Container.Resources`). An unset field is `None`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceRequirements {
    /// Requested CPU in milli-cores; `None` == unset.
    pub cpu_request_milli: Option<u64>,
    /// CPU limit in milli-cores; `None` == unlimited.
    pub cpu_limit_milli: Option<u64>,
    /// Requested memory in bytes; `None` == unset.
    pub memory_request_bytes: Option<u64>,
    /// Memory limit in bytes; `None` == unlimited.
    pub memory_limit_bytes: Option<u64>,
}

impl ResourceRequirements {
    /// The effective requested [`ResourceList`] (unset == 0).
    #[must_use]
    pub fn requests(&self) -> ResourceList {
        ResourceList {
            cpu_milli: self.cpu_request_milli.unwrap_or(0),
            memory_bytes: self.memory_request_bytes.unwrap_or(0),
        }
    }
}

/// Sum the requested resources of a set of containers.
#[must_use]
pub fn sum_requests(containers: &[ResourceRequirements]) -> ResourceList {
    containers
        .iter()
        .fold(ResourceList::default(), |acc, c| acc.add(c.requests()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_is_componentwise() {
        let a = ResourceList::new(500, 1000);
        let b = ResourceList::new(250, 2000);
        assert_eq!(a.add(b), ResourceList::new(750, 3000));
    }

    #[test]
    fn add_saturates() {
        let a = ResourceList::new(u64::MAX, u64::MAX);
        assert_eq!(a.add(ResourceList::new(1, 1)), a);
    }

    #[test]
    fn fits_within_checks_both_axes() {
        let cap = ResourceList::new(1000, 1_000_000);
        assert!(ResourceList::new(1000, 1_000_000).fits_within(cap));
        assert!(!ResourceList::new(1001, 0).fits_within(cap));
        assert!(!ResourceList::new(0, 1_000_001).fits_within(cap));
    }

    #[test]
    fn requests_default_zero() {
        let r = ResourceRequirements::default();
        assert_eq!(r.requests(), ResourceList::new(0, 0));
    }

    #[test]
    fn sum_requests_adds_all() {
        let cs = [
            ResourceRequirements {
                cpu_request_milli: Some(100),
                memory_request_bytes: Some(1000),
                ..Default::default()
            },
            ResourceRequirements {
                cpu_request_milli: Some(200),
                memory_request_bytes: Some(2000),
                ..Default::default()
            },
        ];
        assert_eq!(sum_requests(&cs), ResourceList::new(300, 3000));
    }
}
