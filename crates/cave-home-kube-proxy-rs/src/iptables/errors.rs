// SPDX-License-Identifier: Apache-2.0
//! Error types for the iptables sub-system.
//!
//! Loosely modelled after `pkg/util/iptables/iptables.go` errors but Rust-ified
//! into a single `thiserror`-backed enum so callers can match exhaustively.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxierError {
    /// `iptables-restore` binary returned non-zero exit code.
    #[error("iptables-restore failed (exit={exit_code:?}): {stderr}")]
    IptablesRestoreFailed {
        exit_code: Option<i32>,
        stderr: String,
    },

    /// I/O failure while spawning or talking to `iptables-restore`.
    #[error("iptables-restore I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Could not acquire the `/run/xtables.lock` advisory lock.
    #[error("xtables lock acquisition failed: {0}")]
    LockFailed(String),

    /// Caller invoked the Linux executor on a non-Linux host. Phase 1 is
    /// Linux-only per Charter §6 — see ADR-003.
    #[error("iptables executor unsupported on this platform (Linux-only)")]
    UnsupportedPlatform,

    /// Generic upstream-style "rule generation failed" wrapper.
    #[error("rule generation error: {0}")]
    RuleGeneration(String),
}
