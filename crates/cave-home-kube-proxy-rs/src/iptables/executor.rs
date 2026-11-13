// SPDX-License-Identifier: Apache-2.0
//! `IptablesExecutor` trait + Linux subprocess implementation.
//!
//! Modelled on the documented `iptables-restore --noflush --counters` flow
//! with the `/run/xtables.lock` advisory lock. We split the surface into a
//! trait so the rest of the proxier can be tested without a real iptables
//! binary. Behavioural reimplementation, not a verbatim source port.
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
        drop(st);
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
        Self {
            binary: Some(binary.into()),
        }
    }
}

#[cfg(target_os = "linux")]
impl LinuxExecutor {
    /// Body of [`IptablesExecutor::restore`] with the advisory-lock path as a
    /// parameter, so the locking behaviour can be pinned by tests against an
    /// unprivileged path. Production callers always pass
    /// [`XTABLES_LOCK_PATH`].
    async fn restore_holding_lock(
        &self,
        rules: &str,
        lock_path: &std::path::Path,
    ) -> Result<(), ProxierError> {
        use nix::fcntl::{Flock, FlockArg};
        use std::io::Write as _;
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
            .open(lock_path)
            .map_err(|e| ProxierError::LockFailed(e.to_string()))?;
        // `Flock` takes ownership of the file. If locking fails the file comes
        // back in the error tuple and is closed right there; otherwise the
        // guard lives until this function returns (on every path), where it
        // unlocks and then closes the descriptor exactly once.
        let xtables_lock = Flock::lock(lock_file, FlockArg::LockExclusive)
            .map_err(|(_lock_file, errno)| ProxierError::LockFailed(errno.to_string()))?;

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
        // The lock is released when `xtables_lock` goes out of scope.
        let _ = &xtables_lock;
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

#[cfg(target_os = "linux")]
#[async_trait]
impl IptablesExecutor for LinuxExecutor {
    async fn restore(&self, rules: &str) -> Result<(), ProxierError> {
        self.restore_holding_lock(rules, std::path::Path::new(XTABLES_LOCK_PATH))
            .await
    }
}

#[cfg(not(target_os = "linux"))]
#[async_trait]
impl IptablesExecutor for LinuxExecutor {
    async fn restore(&self, _rules: &str) -> Result<(), ProxierError> {
        Err(ProxierError::UnsupportedPlatform)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use nix::errno::Errno;
    use nix::fcntl::{Flock, FlockArg};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Fresh scratch directory for one test (no privileges needed).
    fn scratch_dir(tag: &str) -> std::io::Result<PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("cave-home-kube-proxy-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Non-blocking probe through a second open file description: `true`
    /// when nobody holds the exclusive lock on `path`.
    fn lock_is_free(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(_probe) => Ok(true),
            Err((_file, errno)) if errno == Errno::EWOULDBLOCK => Ok(false),
            Err((_file, errno)) => Err(errno.into()),
        }
    }

    #[tokio::test]
    async fn unopenable_lock_file_is_a_lock_failure() -> TestResult {
        let dir = scratch_dir("nolock")?;
        let lock_path = dir.join("missing-dir").join("xtables.lock");
        let exec = LinuxExecutor::with_binary("/nonexistent/iptables-restore");
        let err = exec
            .restore_holding_lock("*nat\nCOMMIT\n", &lock_path)
            .await
            .err()
            .ok_or("restore must fail without a lock file")?;
        assert!(matches!(err, ProxierError::LockFailed(_)), "got {err:?}");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    // One test drives both subprocess scenarios so that nothing else in this
    // test binary forks while the stand-in script is being written.
    #[tokio::test]
    async fn restore_holds_the_lock_for_the_whole_call_and_releases_it() -> TestResult {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch_dir("lock")?;
        let lock_path = dir.join("xtables.lock");

        // -- Early return after the lock was taken: spawning fails ----------
        let missing = LinuxExecutor::with_binary(dir.join("no-such-binary").to_string_lossy());
        let err = missing
            .restore_holding_lock("*nat\nCOMMIT\n", &lock_path)
            .await
            .err()
            .ok_or("spawning a missing binary must fail")?;
        assert!(matches!(err, ProxierError::Io(_)), "got {err:?}");
        assert!(lock_path.exists(), "lock file is created on demand");
        assert!(
            lock_is_free(&lock_path)?,
            "lock must be released on the error path"
        );

        // -- Lock is held while the child runs ------------------------------
        // Stand-in for iptables-restore: record stdin, announce itself, then
        // stay alive until the test allows it to exit (bounded wait).
        let script = dir.join("fake-iptables-restore");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             dir=$(dirname \"$0\")\n\
             cat > \"$dir/stdin\"\n\
             : > \"$dir/started\"\n\
             i=0\n\
             while [ ! -e \"$dir/release\" ] && [ \"$i\" -lt 200 ]; do\n\
             \x20 sleep 0.05 2>/dev/null || sleep 1\n\
             \x20 i=$((i + 1))\n\
             done\n",
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;

        let exec = LinuxExecutor::with_binary(script.to_string_lossy());
        let task = tokio::spawn({
            let lock_path = lock_path.clone();
            async move {
                exec.restore_holding_lock("*nat\nCOMMIT\n", &lock_path)
                    .await
            }
        });

        let started = dir.join("started");
        for _ in 0..400 {
            if started.exists() || task.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if !started.exists() {
            if task.is_finished() {
                task.await??;
            }
            return Err("stand-in iptables-restore never started".into());
        }
        assert!(
            !lock_is_free(&lock_path)?,
            "lock must be held while iptables-restore runs"
        );

        std::fs::write(dir.join("release"), b"")?;
        task.await??;

        assert!(
            lock_is_free(&lock_path)?,
            "lock must be released once restore returns"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("stdin"))?,
            "*nat\nCOMMIT\n"
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
