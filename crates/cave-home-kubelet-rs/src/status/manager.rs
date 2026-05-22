// SPDX-License-Identifier: Apache-2.0
//! `PodStatusManager` — pod-status dedup + retry queue.
//!
//! Hand-port of `pkg/kubelet/status/status_manager.go` (v1.36.1) — Phase 1
//! covers the queue + dedup + retry slice; the version-counter (`StatusVersioner`)
//! is `[[unmapped]]` Phase 1b.
//!
//! Behaviour:
//! - `set_pod_status`: store the latest status under the pod UID.
//! - `sync_batch`: for every pod whose cached status differs from the
//!   last-acked status, push it through the sink. On `Transient`, retain
//!   the pending state for the next batch. On `Permanent`, drop it.
//! - `forget_pod`: drop both cached and acked state.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use thiserror::Error;

use super::sink::{StatusSink, StatusSinkError};
use crate::api::{PodStatus, PodUid};

#[derive(Debug, Error)]
pub enum StatusManagerError {
    #[error("sink: {0}")]
    Sink(#[from] StatusSinkError),
}

#[derive(Default)]
struct State {
    /// Latest status set by the kubelet — what we *want* the apiserver to know.
    pending: HashMap<PodUid, PodStatus>,
    /// Last status the sink has accepted.
    acked: HashMap<PodUid, PodStatus>,
}

pub struct PodStatusManager {
    sink: Arc<dyn StatusSink>,
    state: Mutex<State>,
}

impl PodStatusManager {
    #[must_use]
    pub fn new(sink: Arc<dyn StatusSink>) -> Self {
        Self {
            sink,
            state: Mutex::new(State::default()),
        }
    }

    /// Update the cached status. Idempotent if the new status equals the
    /// most-recent pending state.
    pub async fn set_pod_status(
        &self,
        uid: &PodUid,
        status: PodStatus,
    ) -> Result<(), StatusManagerError> {
        self.state.lock().pending.insert(uid.clone(), status);
        Ok(())
    }

    /// Flush every pending update through the sink. Returns the number of
    /// successful writes. Transient failures stay queued for next call.
    pub async fn sync_batch(&self) -> Result<usize, StatusManagerError> {
        // Snapshot pairs to flush so we don't hold the lock across awaits.
        let to_flush: Vec<(PodUid, PodStatus)> = {
            let s = self.state.lock();
            s.pending
                .iter()
                .filter(|(uid, st)| s.acked.get(uid).is_none_or(|a| a != *st))
                .map(|(uid, st)| (uid.clone(), st.clone()))
                .collect()
        };

        let mut succeeded = 0usize;
        for (uid, st) in to_flush {
            match self.sink.write(&uid, &st).await {
                Ok(()) => {
                    self.state.lock().acked.insert(uid, st);
                    succeeded += 1;
                }
                Err(StatusSinkError::Transient(_)) => {
                    // Leave pending; retried next pass.
                }
                Err(StatusSinkError::Permanent(_)) => {
                    // Drop pending so we don't keep retrying a doomed write.
                    self.state.lock().pending.remove(&uid);
                }
            }
        }
        Ok(succeeded)
    }

    /// Drop every trace of a pod (called when the worker enters Terminated).
    pub fn forget_pod(&self, uid: &PodUid) {
        let mut s = self.state.lock();
        s.pending.remove(uid);
        s.acked.remove(uid);
    }

    /// Snapshot the cached pending status (most recent set).
    #[must_use]
    pub fn cached_status(&self, uid: &PodUid) -> Option<PodStatus> {
        self.state.lock().pending.get(uid).cloned()
    }
}
