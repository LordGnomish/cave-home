// SPDX-License-Identifier: Apache-2.0
//! `EndpointSliceCache` — line-by-line port of upstream
//! `pkg/proxy/endpointslicecache.go`.
//!
//! Multiple `EndpointSlice` objects can belong to one Service (via
//! `kubernetes.io/service-name` label). We track them per
//! `(service-namespace/name, slice-name)` and merge into a per-`ServicePortName`
//! endpoint set on snapshot.

use parking_lot::Mutex;
use std::collections::BTreeMap;

use crate::api::{EndpointSlice, NamespacedName, ServicePortName};
use crate::iptables::types::EndpointInfo;

#[derive(Debug, Default)]
struct State {
    /// (svc namespaced name) -> (slice metadata.name) -> slice
    by_service: BTreeMap<NamespacedName, BTreeMap<String, EndpointSlice>>,
    dirty: bool,
}

#[derive(Debug, Default)]
pub struct EndpointSliceCache {
    state: Mutex<State>,
}

impl EndpointSliceCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upstream `OnEndpointSliceAdd` — insert as a fresh slice (idempotent
    /// with `modify`).
    pub fn add(&self, slice: EndpointSlice) {
        self.modify(slice);
    }

    /// Upstream `OnEndpointSliceUpdate` — replace the existing slice.
    pub fn modify(&self, slice: EndpointSlice) {
        let svc_nn = slice.service_namespaced_name();
        let slice_name = slice.metadata.name.clone();
        let mut st = self.state.lock();
        st.by_service
            .entry(svc_nn)
            .or_default()
            .insert(slice_name, slice);
        st.dirty = true;
    }

    /// Upstream `OnEndpointSliceDelete` — remove this slice's entries.
    pub fn delete(&self, slice: &EndpointSlice) {
        let svc_nn = slice.service_namespaced_name();
        let mut st = self.state.lock();
        let (removed, empty) = if let Some(slices) = st.by_service.get_mut(&svc_nn) {
            let r = slices.remove(&slice.metadata.name).is_some();
            (r, slices.is_empty())
        } else {
            (false, false)
        };
        if removed {
            st.dirty = true;
        }
        if empty {
            st.by_service.remove(&svc_nn);
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.state.lock().dirty
    }

    pub fn take_dirty(&self) -> bool {
        let mut st = self.state.lock();
        std::mem::replace(&mut st.dirty, false)
    }

    /// Materialise current cache as a per-`ServicePortName` map of ready
    /// endpoint sockets. Endpoints are deduplicated and sorted.
    #[must_use]
    pub fn snapshot(&self) -> BTreeMap<ServicePortName, Vec<EndpointInfo>> {
        let st = self.state.lock();
        let mut out: BTreeMap<ServicePortName, Vec<EndpointInfo>> = BTreeMap::new();

        for (svc_nn, slices) in &st.by_service {
            for slice in slices.values() {
                // For each port on the slice, build the SPN and walk endpoints.
                for port in &slice.ports {
                    let spn = ServicePortName {
                        namespaced_name: svc_nn.clone(),
                        port: port.name.clone(),
                        protocol: port.protocol,
                    };
                    let bucket = out.entry(spn).or_default();
                    for ep in &slice.endpoints {
                        if !ep.is_ready() {
                            continue;
                        }
                        for ip in &ep.addresses {
                            bucket.push(EndpointInfo { ip: ip.clone(), port: port.port });
                        }
                    }
                }
            }
        }

        // Dedup + sort each bucket for determinism.
        for bucket in out.values_mut() {
            bucket.sort();
            bucket.dedup();
        }
        out
    }
}
