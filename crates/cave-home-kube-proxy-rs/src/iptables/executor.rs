// SPDX-License-Identifier: Apache-2.0
//! `IptablesExecutor` trait + Linux subprocess implementation.
//!
//! Upstream: `pkg/util/iptables/iptables.go` `runner.Restore` /
//! `runner.RestoreAll`. We split the surface into a trait so the rest of
//! the proxier can be tested without a real iptables binary.
//!
//! On non-Linux hosts the `LinuxExecutor` returns
//! `ProxierError::UnsupportedPlatform` — Charter §6 declares Linux 7.1+
//! the ONLY supported deployment target (see ADR-003).

use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::iptables::errors::ProxierError;

/// The subset of `iptables-restore` operations the proxier needs.
/// Returning `Result` lets a real implementation surface non-zero exit
/// codes; the mock variant lets tests prime errors.
#[async_trait]
pub trait IptablesExecutor: Send + Sync {
    /// Run `iptables-restore --noflush --counters` and feed `rules` on stdin.
    async fn restore(&self, rules: &str) -> Result<(), ProxierError>;
}

// ---------------------------------------------------------------------------
// Mock executor — used by the rest of the crate's tests + by downstream
// callers that want to dry-run rule generation.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MockState {
    inputs: Vec<String>,
    next_error: Option<ProxierError>,
}

#[derive(Debug, Clone, Default)]
pub struct MockExecutor {
    state: Arc<Mutex<MockState>>,
}

impl MockExecutor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of every `restore()` payload seen so far, in order.
    #[must_use]
    pub fn recorded_inputs(&self) -> Vec<String> {
        self.state.lock().inputs.clone()
    }

    /// Primes the mock so the NEXT `restore()` call fails with `err`.
    /// Consumed once; subsequent calls succeed unless re-primed.
    pub fn set_next_error(&self, err: ProxierError) {
        self.state.lock().next_error = Some(err);
    }

    /// Discards all recorded inputs — useful between sub-tests.
    pub fn clear(&self) {
        self.state.lock().inputs.clear();
    }
}

#[async_trait]
impl IptablesExecutor for MockExecutor {
    async fn restore(&self, rules: &str) -> Result<(), ProxierError> {
        let mut st = self.state.lock();
        if let Some(err) = st.next_error.take() {
            return Err(err);
        }
        st.inputs.push(rules.to_owned());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Linux subprocess executor — real implementation.
// ---------------------------------------------------------------------------

/// Path to the kernel xtables advisory lock — upstream
/// `pkg/util/iptables/iptables.go LockfilePath16x`.
pub const XTABLES_LOCK_PATH: &str = "/run/xtables.lock";

/// Real iptables-restore wrapper — Linux only.
#[derive(Debug, Clone, Default)]
pub struct LinuxExecutor {
    /// Path to the iptables-restore binary; `None` == lookup on $PATH.
    /// Read only by the `#[cfg(target_os = "linux")]` impl below.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    binary: Option<String>,
}

impl LinuxExecutor {
    #[must_use]
    pub const fn new() -> Self {
        Self { binary: None }
    }

    /// Override the binary path (test hook + `iptables-legacy-restore` users).
    #[must_use]
    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self { binary: Some(binary.into()) }
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl IptablesExecutor for LinuxExecutor {
    async fn restore(&self, rules: &str) -> Result<(), ProxierError> {
        use nix::fcntl::{flock, FlockArg};
        use std::io::Write as _;
        use std::os::fd::AsRawFd as _;
        use tokio::io::AsyncWriteExt as _;
        use tokio::process::Command;

        let bin = self.binary.as_deref().unwrap_or("iptables-restore");

        // -- Acquire /run/xtables.lock (advisory flock) ----------------------
        // Upstream pkg/util/iptables/iptables.go grabIptablesLocks holds the
        // flock for the duration of the restore.
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(XTABLES_LOCK_PATH)
            .map_err(|e| ProxierError::LockFailed(e.to_string()))?;
        flock(lock_file.as_raw_fd(), FlockArg::LockExclusive)
            .map_err(|e| ProxierError::LockFailed(e.to_string()))?;

        // -- Spawn iptables-restore --noflush --counters ---------------------
        let mut child = Command::new(bin)
            .arg("--noflush")
            .arg("--counters")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(rules.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        let output = child.wait_with_output().await?;
        // Drop lock when `lock_file` goes out of scope.
        let _ = lock_file;
        let _ = std::io::sink().write_all(b""); // keep std::io::Write import live

        if output.status.success() {
            Ok(())
        } else {
            Err(ProxierError::IptablesRestoreFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }
}

#[cfg(not(target_os = "linux"))]
#[async_trait]
impl IptablesExecutor for LinuxExecutor {
    async fn restore(&self, _rules: &str) -> Result<(), ProxierError> {
        Err(ProxierError::UnsupportedPlatform)
    }
}
