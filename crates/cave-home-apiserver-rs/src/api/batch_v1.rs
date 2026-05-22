// SPDX-License-Identifier: Apache-2.0
//! batch/v1 resource registry.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! staging/src/k8s.io/api/batch/v1/register.go

/// The kinds the API server serves under `/apis/batch/v1/`.
pub const KINDS: &[(&str, &str, bool)] = &[
    ("jobs", "Job", true),
    ("cronjobs", "CronJob", true),
];

/// Return the kind for a given plural resource, or `None` if unknown.
#[must_use]
pub fn kind_of(resource: &str) -> Option<&'static str> {
    KINDS.iter().find_map(|(r, k, _)| (*r == resource).then_some(*k))
}

/// Whether the resource is namespaced.
#[must_use]
pub fn is_namespaced(resource: &str) -> Option<bool> {
    KINDS.iter().find_map(|(r, _, ns)| (*r == resource).then_some(*ns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_is_namespaced() {
        assert_eq!(is_namespaced("jobs"), Some(true));
        assert_eq!(kind_of("jobs"), Some("Job"));
        assert_eq!(kind_of("cronjobs"), Some("CronJob"));
    }
}
