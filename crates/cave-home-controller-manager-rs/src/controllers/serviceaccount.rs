// SPDX-License-Identifier: Apache-2.0
//! `ServiceAccount` controller — ensures every namespace has a `default`
//! `ServiceAccount`.
//!
//! Behavioural reimplementation of the documented
//! `pkg/controller/serviceaccount` contract: reconcile a namespace key and
//! create the `default` `ServiceAccount` if it is missing (an active namespace
//! only — a terminating one is being torn down).

use crate::apis::{Cluster, ServiceAccount};
use crate::reconcile::Outcome;
use crate::types::ObjectMeta;

/// The conventional name of the auto-created account.
pub const DEFAULT_SERVICE_ACCOUNT: &str = "default";

/// The `ServiceAccount` controller.
#[derive(Debug, Default)]
pub struct ServiceAccountController;

impl ServiceAccountController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one namespace key (`"<name>"`); ensure its `default` SA exists.
    pub fn reconcile(&mut self, namespace: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(ns) = cluster.namespaces.get(namespace) else {
            return Outcome::Done;
        };
        if ns.meta.is_terminating() {
            return Outcome::Done;
        }
        let key = format!("{namespace}/{DEFAULT_SERVICE_ACCOUNT}");
        if cluster.service_accounts.get(&key).is_none() {
            let meta = ObjectMeta::new(DEFAULT_SERVICE_ACCOUNT, namespace, "");
            cluster.service_accounts.create(ServiceAccount::new(meta));
        }
        Outcome::Done
    }
}
