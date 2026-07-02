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

use cave_home_orchestration::secrets_encryption::status::EncryptionStatus;

/// The command path this surface lives under.
#[must_use]
pub const fn command_path() -> &'static str {
    "orchestration secrets encryption"
}

/// The leaf subcommands under [`command_path`].
#[must_use]
pub fn subcommands() -> Vec<&'static str> {
    vec!["status", "rotate-keys"]
}

/// Render the `status` output from the backend view-model.
#[must_use]
pub fn render_status(status: &EncryptionStatus) -> String {
    let state = if status.enabled { "enabled" } else { "disabled" };
    format!(
        "Secrets encryption-at-rest\n  \
         status:    {state}\n  \
         algorithm: {}\n  \
         write key: {}\n  \
         keys:      {} ({} retained read-only)\n  \
         rotation:  {:?}\n",
        status.algorithm,
        status.write_key_id,
        status.key_count,
        status.read_only_key_count,
        status.rotation_phase,
    )
}

/// Render the result of a `rotate-keys` action — the new write key + phase.
///
/// `status` is the post-rotation snapshot; the runtime still has to re-encrypt
/// every secret and prune the old keys (this surface decides + reports; the
/// datastore re-write is ADR-004 phase-1b).
#[must_use]
pub fn render_rotate_keys(status: &EncryptionStatus) -> String {
    format!(
        "Rotated secrets encryption keys.\n  \
         new write key: {}\n  \
         rotation:      {:?}\n  \
         next:          re-encrypt secrets, then prune {} stale key(s)\n",
        status.write_key_id, status.rotation_phase, status.read_only_key_count,
    )
}

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
