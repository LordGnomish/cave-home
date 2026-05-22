// SPDX-License-Identifier: Apache-2.0
//! Portal hook for the controller-manager admin surface.
//!
//! Phase 2b will mount a Lovelace-class panel here that talks to
//! `cave-home-controller-manager-rs` (per-controller workqueue depth,
//! reconcile latency, last-error tails for Deployment / RS / DS / STS /
//! Job / CronJob / SA / Namespace / Node / GC) via the apiserver; for
//! Phase 2 this is a compiling placeholder so the 4-track gate is
//! satisfied.

#[must_use]
pub fn admin_routes_placeholder() -> &'static str {
    "/admin/controllers — Phase 2b"
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_routes_path_is_stable() {
        assert_eq!(admin_routes_placeholder(), "/admin/controllers — Phase 2b");
    }
}
