// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Error type for the tracker.

use std::path::PathBuf;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, TrackerError>;

/// Everything that can go wrong while polling, measuring or reporting.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TrackerError {
    /// An I/O operation failed, annotated with the path it concerned.
    #[error("io error at {path}: {source}")]
    Io {
        /// Path the failing operation touched.
        path: PathBuf,
        /// Underlying OS error.
        source: std::io::Error,
    },

    /// A bare I/O error with no useful path context.
    #[error("io error: {0}")]
    Bare(#[from] std::io::Error),

    /// Failed to parse the `tracker.yaml` configuration.
    #[error("config parse error: {0}")]
    Config(#[from] serde_yaml::Error),

    /// Failed to (de)serialise a snapshot as JSON.
    #[error("snapshot json error: {0}")]
    Json(#[from] serde_json::Error),

    /// An external command (`git`, `cargo`) exited non-zero.
    #[error("command `{cmd}` failed ({code}): {stderr}")]
    Command {
        /// The command line that was run.
        cmd: String,
        /// Exit code, or `-1` when the process was killed by a signal.
        code: i32,
        /// Captured standard error.
        stderr: String,
    },

    /// The configuration referenced something that does not exist.
    #[error("not found: {0}")]
    NotFound(String),
}

impl TrackerError {
    /// Build an [`TrackerError::Io`] carrying the offending `path`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Build a [`TrackerError::Command`] failure.
    pub fn command(cmd: impl Into<String>, code: i32, stderr: impl Into<String>) -> Self {
        Self::Command {
            cmd: cmd.into(),
            code,
            stderr: stderr.into(),
        }
    }
}
