// SPDX-License-Identifier: Apache-2.0
//! Endpoints controller — keeps a Service's Endpoints object in sync with its
//! ready backing pods.
//!
//! Behavioural reimplementation of the documented `pkg/controller/endpoint`
//! contract, reconciling against the in-memory apiserver: for a Service, select
//! the ready, active pods matching its selector and write their addresses into
//! an [`Endpoints`](crate::apis::Endpoints) object of the same name. The full
//! `EndpointSlice` machinery and per-port subsets are deferred; this models the
//! ready-address set, which is the load-bearing decision.

use crate::apis::{Cluster, Endpoints};
use crate::reconcile::Outcome;
use crate::types::{Object, ObjectMeta};

/// The Endpoints controller.
#[derive(Debug, Default)]
pub struct EndpointsController;

impl EndpointsController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one Service key (`"<ns>/<name>"`).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(svc) = cluster.services.get(key) else {
            return Outcome::Done;
        };
        let ns = &svc.meta.namespace;

        let mut addresses: Vec<String> = cluster
            .pods
            .list_matching(ns, &svc.selector)
            .into_iter()
            .filter(|p| p.is_active() && p.status.ready)
            .map(|p| p.key())
            .collect();
        addresses.sort();
        addresses.dedup();

        let ep = cluster
            .endpoints
            .get(key)
            .map_or_else(|| Endpoints::new(ObjectMeta::new(&svc.meta.name, ns, "")), |mut e| {
                e.addresses.clear();
                e
            });
        let mut ep = ep;
        ep.addresses = addresses;
        // create() assigns a UID on first write; update() replaces thereafter.
        if cluster.endpoints.get(key).is_some() {
            cluster.endpoints.update(ep);
        } else {
            cluster.endpoints.create(ep);
        }
        Outcome::Done
    }
}
