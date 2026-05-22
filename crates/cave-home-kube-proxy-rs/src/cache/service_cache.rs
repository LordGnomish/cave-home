// SPDX-License-Identifier: Apache-2.0
//! `ServiceCache` — line-by-line port of upstream `pkg/proxy/serviceconfig.go`
//! + `pkg/proxy/servicechangetracker.go`.
//!
//! The upstream design splits add/update/delete tracking from the snapshot
//! generation; here we collapse the two because Phase 1 has no resync delta
//! optimisation (full sync is fine at our scale — see ROADMAP M2.5).

use parking_lot::Mutex;
use std::collections::BTreeMap;

use crate::api::{NamespacedName, Service};
use crate::iptables::types::ServicePortInfo;

#[derive(Debug, Default)]
struct State {
    services: BTreeMap<NamespacedName, Service>,
    dirty: bool,
}

/// Thread-safe (`Mutex`) cache of all observed `Service` objects.
#[derive(Debug, Default)]
pub struct ServiceCache {
    state: Mutex<State>,
}

impl ServiceCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upstream `OnServiceAdd`. Idempotent — re-adding the same key is a modify.
    pub fn add(&self, svc: Service) {
        self.modify(svc);
    }

    /// Upstream `OnServiceUpdate`.
    pub fn modify(&self, svc: Service) {
        let mut st = self.state.lock();
        st.services.insert(svc.metadata.clone(), svc);
        st.dirty = true;
    }

    /// Upstream `OnServiceDelete`.
    pub fn delete(&self, svc: &Service) {
        let mut st = self.state.lock();
        if st.services.remove(&svc.metadata).is_some() {
            st.dirty = true;
        }
    }

    /// `true` iff there are unprocessed changes since the last `take_dirty`.
    pub fn is_dirty(&self) -> bool {
        self.state.lock().dirty
    }

    /// Returns the current dirty state and clears the flag in one atomic step.
    /// Used by the reconciler to decide whether to re-sync.
    pub fn take_dirty(&self) -> bool {
        let mut st = self.state.lock();
        std::mem::replace(&mut st.dirty, false)
    }

    /// Snapshot all proxiable services as `ServicePortInfo` rows
    /// (one per Service x ServicePort), filtering with `ShouldSkipService`
    /// and sorting deterministically.
    #[must_use]
    pub fn snapshot(&self) -> Vec<ServicePortInfo> {
        let st = self.state.lock();
        let mut out: Vec<ServicePortInfo> = Vec::new();
        for svc in st.services.values() {
            if svc.should_skip() {
                continue;
            }
            for sp in &svc.ports {
                out.push(ServicePortInfo {
                    name: crate::api::ServicePortName {
                        namespaced_name: svc.metadata.clone(),
                        port: sp.name.clone(),
                        protocol: sp.protocol,
                    },
                    cluster_ip: svc.cluster_ip.clone(),
                    port: sp.port,
                    protocol: sp.protocol,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}
