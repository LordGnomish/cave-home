// SPDX-License-Identifier: Apache-2.0
//! Portal hook for the apiserver admin surface.
//!
//! Phase 2b will mount a Lovelace-class panel here that talks to
//! `cave-home-apiserver-rs` (request rate, watch-cache depth, admission
//! decisions, RBAC denials, storage RV lag) via the apiserver itself;
//! for Phase 2 this is a compiling placeholder so the 4-track gate is
//! satisfied.

#[must_use]
pub fn admin_routes_placeholder() -> &'static str {
    "/admin/apiserver — Phase 2b"
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_routes_path_is_stable() {
        assert_eq!(admin_routes_placeholder(), "/admin/apiserver — Phase 2b");
    }
}
