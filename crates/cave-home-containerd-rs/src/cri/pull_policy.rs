// SPDX-License-Identifier: Apache-2.0
//! Image-pull-policy decision.
//!
//! Behavioural reimplementation of the documented kubelet image-pull policy:
//! given the policy and whether the image is already present locally, decide
//! whether to pull, use the local copy, or fail.
//!
//! Spec source: Kubernetes container-image pull-policy semantics
//!   * `Always`       — always attempt a pull.
//!   * `IfNotPresent` — pull only if the image is not already present.
//!   * `Never`        — never pull; fail if the image is absent.

/// The kubelet image-pull policy for a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullPolicy {
    /// Always attempt to pull the image.
    Always,
    /// Pull only when the image is not present locally.
    IfNotPresent,
    /// Never pull; rely solely on a locally-present image.
    Never,
}

/// The outcome of a pull-policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullDecision {
    /// Pull the image from the registry before starting the container.
    Pull,
    /// Use the image already present locally; no pull needed.
    UseLocal,
    /// The image is absent and the policy forbids pulling — fail.
    ErrImageNeverPull,
}

/// Decides what to do for an image given the policy and local presence.
///
/// ```
/// use cave_home_containerd_rs::cri::pull_policy::{decide_pull, PullPolicy, PullDecision};
/// assert_eq!(decide_pull(PullPolicy::IfNotPresent, true), PullDecision::UseLocal);
/// assert_eq!(decide_pull(PullPolicy::Never, false), PullDecision::ErrImageNeverPull);
/// ```
#[must_use]
pub const fn decide_pull(policy: PullPolicy, image_present: bool) -> PullDecision {
    match policy {
        PullPolicy::Always => PullDecision::Pull,
        PullPolicy::IfNotPresent => {
            if image_present {
                PullDecision::UseLocal
            } else {
                PullDecision::Pull
            }
        }
        PullPolicy::Never => {
            if image_present {
                PullDecision::UseLocal
            } else {
                PullDecision::ErrImageNeverPull
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_pulls_regardless_of_presence() {
        assert_eq!(decide_pull(PullPolicy::Always, true), PullDecision::Pull);
        assert_eq!(decide_pull(PullPolicy::Always, false), PullDecision::Pull);
    }

    #[test]
    fn if_not_present_uses_local_when_present() {
        assert_eq!(decide_pull(PullPolicy::IfNotPresent, true), PullDecision::UseLocal);
    }

    #[test]
    fn if_not_present_pulls_when_absent() {
        assert_eq!(decide_pull(PullPolicy::IfNotPresent, false), PullDecision::Pull);
    }

    #[test]
    fn never_uses_local_when_present() {
        assert_eq!(decide_pull(PullPolicy::Never, true), PullDecision::UseLocal);
    }

    #[test]
    fn never_errors_when_absent() {
        assert_eq!(decide_pull(PullPolicy::Never, false), PullDecision::ErrImageNeverPull);
    }
}
