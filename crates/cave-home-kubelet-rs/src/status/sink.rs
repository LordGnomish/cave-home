// SPDX-License-Identifier: Apache-2.0
//! `StatusSink` trait — abstraction over the apiserver UpdatePodStatus call.
//!
//! Hand-port of the `kubeClient` interface used by
//! `pkg/kubelet/status/status_manager.go::syncPod`.
//!
//! Real apiserver wiring is `[[unmapped]]` Phase 1b (workspace-level
//! integration concern, needs kubeconfig + mTLS + http client).

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;
use thiserror::Error;

use crate::api::{PodStatus, PodUid};

/// Sink error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum StatusSinkError {
    /// Network or apiserver-side failure — caller will retry.
    #[error("transient: {0}")]
    Transient(String),
    /// Permanent failure (e.g. 404 — pod is gone).
    #[error("permanent: {0}")]
    Permanent(String),
}

/// Sink that accepts a single pod-status update.
#[async_trait]
pub trait StatusSink: Send + Sync {
    async fn write(&self, uid: &PodUid, status: &PodStatus) -> Result<(), StatusSinkError>;
}

/// Deterministic in-memory sink used by status-manager tests.
pub struct MockStatusSink {
    /// Last status accepted per pod UID.
    inner: Mutex<Vec<(PodUid, PodStatus)>>,
    /// When true, every `write` returns `Transient` once before succeeding.
    fail_once: AtomicBool,
    /// When true, every `write` returns `Transient` forever.
    always_fail: AtomicBool,
}

impl Default for MockStatusSink {
    fn default() -> Self {
        Self::new()
    }
}

impl MockStatusSink {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
            fail_once: AtomicBool::new(false),
            always_fail: AtomicBool::new(false),
        }
    }

    pub fn set_fail_once(&self) {
        self.fail_once.store(true, Ordering::SeqCst);
    }

    pub fn set_always_fail(&self, on: bool) {
        self.always_fail.store(on, Ordering::SeqCst);
    }

    /// Snapshot of every accepted (uid, status) write, in order.
    pub fn writes(&self) -> Vec<(PodUid, PodStatus)> {
        self.inner.lock().clone()
    }

    pub fn write_count(&self) -> usize {
        self.inner.lock().len()
    }
}

#[async_trait]
impl StatusSink for MockStatusSink {
    async fn write(&self, uid: &PodUid, status: &PodStatus) -> Result<(), StatusSinkError> {
        if self.always_fail.load(Ordering::SeqCst) {
            return Err(StatusSinkError::Transient("always_fail".into()));
        }
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(StatusSinkError::Transient("fail_once".into()));
        }
        self.inner.lock().push((uid.clone(), status.clone()));
        Ok(())
    }
}
