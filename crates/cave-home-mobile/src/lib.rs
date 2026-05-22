// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cave-home-mobile — Rust shim crate for the Flutter companion app.
//!
//! ADR-006 (b) — Flutter is the recommended mobile stack. The actual
//! mobile app is a Flutter project located at:
//!
//!   `crates/cave-home-mobile/flutter_app/`
//!
//! See that directory's `README.md` for the Flutter SDK requirements
//! and feature coverage matrix.
//!
//! This Rust shim exists so:
//!   * the workspace `Cargo.toml` can list `cave-home-mobile` like
//!     every other cave-home crate, and
//!   * Phase 2b can publish FFI helpers here (via
//!     `flutter-rust-bridge`) without re-plumbing the workspace.

/// Path (relative to repo root) to the Flutter project that owns the
/// real mobile UI. Surfaces in build scripts / CI runners.
pub const FLUTTER_APP_DIR: &str = "crates/cave-home-mobile/flutter_app";

/// Phase 2b will replace this with a real FFI surface bridged into the
/// Flutter app. For now it's an explicit marker so consumers can ask
/// "does this build of cave-home expose a mobile bridge?".
#[must_use]
pub const fn flutter_bridge_present() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flutter_app_dir_points_at_flutter_project() {
        assert!(FLUTTER_APP_DIR.contains("flutter_app"));
    }

    #[test]
    fn bridge_not_present_yet() {
        assert!(!flutter_bridge_present());
    }
}
