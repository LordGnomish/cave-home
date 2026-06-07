// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl orchestration secrets encryption {status|rotate-keys}` — the
//! power-user surface over `cave-home-orchestration`'s secrets
//! encryption-at-rest decision core.
//!
//! Like the other orchestration CLI modules this is **hidden infrastructure**
//! (ADR-007 §6.3): it carries no home-world vocabulary and is not wired into the
//! end-user `cavehomectl` command tree. It renders the backend's
//! `EncryptionStatus` view-model so the surface tracks the real data contract
//! rather than a stub. The live datastore/apiserver wiring that would feed it a
//! running keyring is ADR-004 phase-1b.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use cave_home_orchestration::secrets_encryption::keyring::RotationPhase;
    use cave_home_orchestration::secrets_encryption::status::EncryptionStatus;

    fn status(enabled: bool, phase: RotationPhase, write_key: &str) -> EncryptionStatus {
        EncryptionStatus {
            enabled,
            algorithm: "ML-KEM-768+AES-256-GCM",
            write_key_id: write_key.to_owned(),
            key_count: if matches!(phase, RotationPhase::Steady) { 1 } else { 2 },
            read_only_key_count: if matches!(phase, RotationPhase::Steady) { 0 } else { 1 },
            rotation_phase: phase,
        }
    }

    #[test]
    fn lists_status_and_rotate_keys_subcommands() {
        let sc = subcommands();
        assert!(sc.contains(&"status"));
        assert!(sc.contains(&"rotate-keys"));
    }

    #[test]
    fn command_path_is_orchestration_secrets_encryption() {
        assert_eq!(command_path(), "orchestration secrets encryption");
    }

    #[test]
    fn render_status_shows_enabled_algorithm_key_and_phase() {
        let out = render_status(&status(true, RotationPhase::Steady, "key-1"));
        assert!(out.contains("enabled"));
        assert!(out.contains("ML-KEM-768+AES-256-GCM"));
        assert!(out.contains("key-1"));
        assert!(out.contains("Steady"));
    }

    #[test]
    fn render_status_shows_disabled() {
        let out = render_status(&status(false, RotationPhase::Steady, "key-1"));
        assert!(out.contains("disabled"));
    }

    #[test]
    fn render_rotate_keys_reports_new_write_key_and_phase() {
        let out = render_rotate_keys(&status(true, RotationPhase::Rotated, "key-2"));
        assert!(out.contains("key-2"));
        assert!(out.contains("Rotated"));
    }
}
