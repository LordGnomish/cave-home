// SPDX-License-Identifier: Apache-2.0
//! Kubernetes Ingress controller: reconcile Ingress objects into a validated
//! routing config and hot-swap it into service.
//!
//! Spec basis: Traefik's `kubernetes-ingress` provider watches Ingress (and
//! their backing Services/Endpoints), translates them into the dynamic
//! configuration, and atomically replaces the live config on every change.
//!
//! The translation reuses [`crate::ingress`]; this adds the reconcile step
//! (resolving backends to servers) and a thread-safe [`ConfigHolder`] the async
//! listener reads on each request. The watch *stream* that calls
//! [`reconcile`] on change is the listener's job; the reconcile itself is pure.

use std::sync::{Arc, RwLock};

use crate::config::{ConfigError, DynamicConfig};
use crate::ingress::{translate_ingresses, Ingress, IngressBackend, IngressError};
use crate::loadbalancer::Server;

/// An error reconciling Ingress objects into a config snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileError {
    /// Translating the Ingress objects failed.
    Translate(IngressError),
    /// The resulting config failed validation.
    Build(ConfigError),
}

impl std::fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Translate(e) => write!(f, "ingress translation failed: {e}"),
            Self::Build(e) => write!(f, "config build failed: {e}"),
        }
    }
}

impl std::error::Error for ReconcileError {}

/// Reconcile a set of Ingress objects into a validated [`DynamicConfig`].
///
/// `controller_class` filters which ingresses this controller owns; `resolve`
/// maps each backend to its concrete server pool (see [`crate::discovery`]).
///
/// # Errors
/// [`ReconcileError`] if translation or validation fails.
pub fn reconcile<F>(
    ingresses: &[Ingress],
    controller_class: Option<&str>,
    resolve: F,
) -> Result<DynamicConfig, ReconcileError>
where
    F: Fn(&IngressBackend) -> Vec<Server>,
{
    let translation =
        translate_ingresses(ingresses, controller_class).map_err(ReconcileError::Translate)?;
    translation.into_config(resolve).map_err(ReconcileError::Build)
}

/// A thread-safe holder for the live config, swapped atomically on reconcile.
#[derive(Debug)]
pub struct ConfigHolder {
    inner: RwLock<Arc<DynamicConfig>>,
}

impl ConfigHolder {
    /// Wrap an initial config.
    #[must_use]
    pub fn new(config: DynamicConfig) -> Self {
        Self { inner: RwLock::new(Arc::new(config)) }
    }

    /// Load the current config snapshot (cheap `Arc` clone).
    #[must_use]
    pub fn load(&self) -> Arc<DynamicConfig> {
        let guard = self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone()
    }

    /// Atomically replace the live config.
    pub fn store(&self, config: DynamicConfig) {
        let mut guard = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Arc::new(config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingress::{HttpPath, IngressRule, PathType};
    use crate::request::RequestDescriptor;

    fn ingress_for(host: &str, service: &str, port: u16) -> Ingress {
        let backend = IngressBackend::numeric(service, port);
        let path = HttpPath::new(Some("/"), PathType::Prefix, backend);
        Ingress {
            namespace: "default".to_string(),
            name: "web".to_string(),
            ingress_class_name: None,
            default_backend: None,
            rules: vec![IngressRule::new(Some(host), vec![path])],
            tls: Vec::new(),
        }
    }

    #[test]
    fn reconcile_builds_routable_config() {
        let ingresses = vec![ingress_for("app.example", "web-svc", 80)];
        let config = reconcile(&ingresses, None, |backend| {
            assert_eq!(backend.service_name, "web-svc");
            vec![Server::new("http://10.0.0.1:80")]
        })
        .unwrap();
        let req = RequestDescriptor::new("GET", "http", "app.example", "/");
        let route = config.route(&req, None).expect("a route matches");
        assert_eq!(route.service.servers[0].url, "http://10.0.0.1:80");
    }

    #[test]
    fn reconcile_propagates_empty_backend_as_error() {
        let ingresses = vec![ingress_for("app.example", "web-svc", 80)];
        // A resolver that returns no servers must surface a validation error,
        // not silently produce an empty service.
        let err = reconcile(&ingresses, None, |_| Vec::new()).unwrap_err();
        assert!(matches!(err, ReconcileError::Build(_)));
    }

    #[test]
    fn config_holder_swaps_atomically() {
        let empty = DynamicConfig::default();
        let holder = ConfigHolder::new(empty);
        assert!(holder.load().routers().is_empty());

        let cfg = reconcile(&[ingress_for("a.example", "svc", 80)], None, |_| {
            vec![Server::new("http://1.1.1.1:80")]
        })
        .unwrap();
        holder.store(cfg);
        assert_eq!(holder.load().routers().len(), 1);
    }
}
