// SPDX-License-Identifier: Apache-2.0
//! `GenericPleg::relist` + diff.
//!
//! Hand-port of `pkg/kubelet/pleg/generic.go::Relist`.
//!
//! Algorithm (verbatim from upstream):
//! 1. Snapshot current pod-sandboxes + containers via the CRI client.
//! 2. Bucket containers by sandbox UID (we use the sandbox metadata.uid as
//!    the pod UID — the kubelet sets it to `Pod.metadata.uid`).
//! 3. Diff the new snapshot against the cached `PodRecords`:
//!    - `Created` -> `Running`: emit `ContainerStarted`.
//!    - `Running` -> `Exited`:  emit `ContainerDied`.
//!    - present-in-old, absent-from-new: emit `ContainerRemoved`.
//!    - sandbox UID appears or disappears: emit `PodSync`.
//! 4. Persist the new snapshot.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::broadcast;

use super::clock::Clock;
use super::pod_record::{ContainerSnapshot, PodRecord, PodRecords};
use super::types::{PodLifecycleEvent, PodLifecycleEventType};
use crate::api::PodUid;
use crate::cri::CriClient;
use crate::cri::types::ContainerState as CriContainerState;

/// Default broadcast capacity. The kubelet runs N pod-workers, each owning
/// one receiver end; 256 is enough headroom for the burstiest relist.
pub const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 256;

/// `GenericPleg` — owns the relist cache and the broadcast channel.
pub struct GenericPleg {
    cri: Arc<dyn CriClient>,
    clock: Arc<dyn Clock>,
    records: Mutex<PodRecords>,
    last_relist_ms: Mutex<i64>,
    tx: broadcast::Sender<PodLifecycleEvent>,
}

impl GenericPleg {
    /// Construct a new PLEG.
    #[must_use]
    pub fn new(cri: Arc<dyn CriClient>, clock: Arc<dyn Clock>) -> Self {
        let (tx, _rx) = broadcast::channel(DEFAULT_EVENT_CHANNEL_CAPACITY);
        Self {
            cri,
            clock,
            records: Mutex::new(PodRecords::new()),
            last_relist_ms: Mutex::new(0),
            tx,
        }
    }

    /// Subscribe to lifecycle events. Each subscriber owns its own queue.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<PodLifecycleEvent> {
        self.tx.subscribe()
    }

    /// Wall-clock millis at which the last successful relist completed.
    pub fn last_relist_ms(&self) -> i64 {
        *self.last_relist_ms.lock()
    }

    /// Relist & diff. Returns the number of events emitted.
    pub async fn relist(&self) -> usize {
        // 1. Snapshot.
        let sandboxes = match self.cri.list_pod_sandbox(None).await {
            Ok(v) => v,
            Err(_) => return 0,
        };
        let containers = match self.cri.list_containers(None).await {
            Ok(v) => v,
            Err(_) => return 0,
        };

        // 2. Bucket containers by sandbox UID. We need a sandbox-id -> uid
        //    side-table because containers reference the sandbox by ID, not
        //    by UID.
        let mut sandbox_uid_by_id: HashMap<String, PodUid> = HashMap::new();
        let mut current_uids: HashSet<PodUid> = HashSet::new();
        for sb in &sandboxes {
            let uid = PodUid::new(&sb.metadata.uid);
            sandbox_uid_by_id.insert(sb.id.clone(), uid.clone());
            current_uids.insert(uid);
        }

        let mut new_records: PodRecords = HashMap::new();
        for sb in &sandboxes {
            let uid = PodUid::new(&sb.metadata.uid);
            new_records.entry(uid.clone()).or_insert_with(|| PodRecord {
                uid,
                containers: Vec::new(),
            });
        }
        for c in &containers {
            let Some(uid) = sandbox_uid_by_id.get(&c.pod_sandbox_id) else {
                continue;
            };
            let entry = new_records.entry(uid.clone()).or_insert_with(|| PodRecord {
                uid: uid.clone(),
                containers: Vec::new(),
            });
            entry.containers.push(ContainerSnapshot {
                id: c.id.clone(),
                state: c.state,
            });
        }

        // 3. Diff.
        let mut events: Vec<PodLifecycleEvent> = Vec::new();
        let old = self.records.lock().clone();

        // Track per-pod container event emission so we know whether to also
        // emit a PodSync for a sandbox-only change.
        let mut emitted_for: HashSet<PodUid> = HashSet::new();

        // Per-pod container diff.
        for (uid, new_rec) in &new_records {
            let empty_rec = PodRecord {
                uid: uid.clone(),
                containers: Vec::new(),
            };
            let old_rec = old.get(uid).unwrap_or(&empty_rec);
            // a) per-container transitions for containers present in `new_rec`.
            for nc in &new_rec.containers {
                let prior = old_rec.container(&nc.id);
                match prior {
                    None => {
                        // Brand new container.
                        if nc.state == CriContainerState::Running {
                            events.push(PodLifecycleEvent {
                                id: uid.clone(),
                                container_id: nc.id.clone(),
                                event_type: PodLifecycleEventType::ContainerStarted,
                            });
                            emitted_for.insert(uid.clone());
                        }
                    }
                    Some(prev) => {
                        if prev.state == CriContainerState::Created
                            && nc.state == CriContainerState::Running
                        {
                            events.push(PodLifecycleEvent {
                                id: uid.clone(),
                                container_id: nc.id.clone(),
                                event_type: PodLifecycleEventType::ContainerStarted,
                            });
                            emitted_for.insert(uid.clone());
                        } else if prev.state == CriContainerState::Running
                            && nc.state == CriContainerState::Exited
                        {
                            events.push(PodLifecycleEvent {
                                id: uid.clone(),
                                container_id: nc.id.clone(),
                                event_type: PodLifecycleEventType::ContainerDied,
                            });
                            emitted_for.insert(uid.clone());
                        } else if prev.state != nc.state {
                            events.push(PodLifecycleEvent {
                                id: uid.clone(),
                                container_id: nc.id.clone(),
                                event_type: PodLifecycleEventType::ContainerChanged,
                            });
                            emitted_for.insert(uid.clone());
                        }
                    }
                }
            }
            // b) Containers present-in-old, absent-from-new -> Removed.
            for oc in &old_rec.containers {
                if new_rec.container(&oc.id).is_none() {
                    events.push(PodLifecycleEvent {
                        id: uid.clone(),
                        container_id: oc.id.clone(),
                        event_type: PodLifecycleEventType::ContainerRemoved,
                    });
                    emitted_for.insert(uid.clone());
                }
            }
        }
        // c) Containers belonging to pods that fully disappeared (no entry
        //    in `new_records`) - emit ContainerRemoved per container.
        for (old_uid, old_rec) in &old {
            if !new_records.contains_key(old_uid) {
                for oc in &old_rec.containers {
                    events.push(PodLifecycleEvent {
                        id: old_uid.clone(),
                        container_id: oc.id.clone(),
                        event_type: PodLifecycleEventType::ContainerRemoved,
                    });
                    emitted_for.insert(old_uid.clone());
                }
            }
        }
        // d) Sandbox UIDs that newly appeared OR fully disappeared with no
        //    container event yet -> PodSync (sandbox-level transition).
        for old_uid in old.keys() {
            if !current_uids.contains(old_uid) && !emitted_for.contains(old_uid) {
                events.push(PodLifecycleEvent {
                    id: old_uid.clone(),
                    container_id: String::new(),
                    event_type: PodLifecycleEventType::PodSync,
                });
            }
        }
        for new_uid in &current_uids {
            if !old.contains_key(new_uid) && !emitted_for.contains(new_uid) {
                events.push(PodLifecycleEvent {
                    id: new_uid.clone(),
                    container_id: String::new(),
                    event_type: PodLifecycleEventType::PodSync,
                });
            }
        }

        // 4. Persist & broadcast.
        *self.records.lock() = new_records;
        *self.last_relist_ms.lock() = self.clock.now_unix_millis();
        let mut emitted = 0usize;
        for e in events {
            // `send` only fails when there are no live subscribers; in that
            // case we still advance the cache (parity with upstream behaviour).
            if self.tx.send(e).is_ok() {
                emitted += 1;
            } else {
                emitted += 1;
            }
        }
        emitted
    }
}
