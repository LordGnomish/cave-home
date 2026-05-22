// SPDX-License-Identifier: Apache-2.0
//! Portal hook for the scheduler admin surface.
//!
//! Phase 2b will mount a Lovelace-class panel here that talks to
//! `cave-home-scheduler-rs` (queue depth, last decisions, plugin
//! result histograms) via the apiserver; for Phase 2 this is a
//! compiling placeholder.

#[must_use]
pub fn admin_routes_placeholder() -> &'static str {
    "/admin/scheduler — Phase 2b"
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_routes_path_is_stable() {
        assert_eq!(admin_routes_placeholder(), "/admin/scheduler — Phase 2b");
    }
}
