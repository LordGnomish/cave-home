//! Port of `homeassistant.loader` + the `homeassistant.setup` ordering logic.
//!
//! An [`Integration`] is a plugin: a [`Manifest`] (domain, friendly name, hard
//! `dependencies`, soft `after_dependencies`, pip-style `requirements`) plus a
//! `setup` hook handed the shared [`CoreContext`]. This is the seam external
//! crates — `cave-home-freeathome`, `cave-home-unifi`, `cave-home-hue` — plug
//! into: each ships a type implementing [`Integration`] and registers it.
//!
//! [`IntegrationLoader`] is also the *domain registry*: it owns the set of
//! known domains, computes a dependency-respecting [`setup_order`] (rejecting
//! missing hard dependencies and dependency cycles), and drives
//! [`setup_all`], skipping any integration whose hard dependency failed —
//! exactly the cascade HA's `async_setup_component` implements.
//!
//! [`setup_order`]: IntegrationLoader::setup_order
//! [`setup_all`]: IntegrationLoader::setup_all

use crate::core_context::CoreContext;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LoaderError {
    #[error("domain {0:?} is already registered")]
    DuplicateDomain(String),
    #[error("integration {domain:?} depends on {dependency:?}, which is not registered")]
    MissingDependency { domain: String, dependency: String },
    #[error("dependency cycle: {0:?}")]
    DependencyCycle(Vec<String>),
}

/// Raised by an [`Integration::setup`] that cannot start.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("setup of {domain:?} failed: {reason}")]
pub struct SetupError {
    pub domain: String,
    pub reason: String,
}

impl SetupError {
    pub fn new(domain: impl Into<String>, reason: impl Into<String>) -> Self {
        Self { domain: domain.into(), reason: reason.into() }
    }
}

/// Port of `homeassistant.loader.Manifest` (the fields the loader consumes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub domain: String,
    pub name: String,
    /// Hard dependencies — must be registered and set up first.
    pub dependencies: Vec<String>,
    /// Soft dependencies — set up first *if present*, ignored otherwise.
    pub after_dependencies: Vec<String>,
    /// pip-style requirement strings (recorded, not resolved here).
    pub requirements: Vec<String>,
}

impl Manifest {
    /// A manifest with just a domain and name.
    #[must_use]
    pub fn new(domain: impl Into<String>, name: impl Into<String>) -> Self {
        Self { domain: domain.into(), name: name.into(), ..Self::default() }
    }

    /// Builder: set hard dependencies.
    #[must_use]
    pub fn with_dependencies(mut self, deps: &[&str]) -> Self {
        self.dependencies = deps.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    /// Builder: set soft (after) dependencies.
    #[must_use]
    pub fn with_after_dependencies(mut self, deps: &[&str]) -> Self {
        self.after_dependencies = deps.iter().map(|s| (*s).to_owned()).collect();
        self
    }
}

/// Port of a loadable integration. The setup hook returns `Ok(true)` on
/// success, `Ok(false)` for a clean "could not set up" (HA's convention), or an
/// [`SetupError`].
pub trait Integration: Send + Sync {
    fn manifest(&self) -> &Manifest;

    /// # Errors
    /// [`SetupError`] if the integration fails to initialise.
    fn setup(&self, ctx: &CoreContext) -> Result<bool, SetupError>;
}

/// Outcome of [`IntegrationLoader::setup_all`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetupReport {
    /// Domains whose `setup` returned `Ok(true)`, in setup order.
    pub set_up: Vec<String>,
    /// Domains that failed, mapped to a human reason. A domain fails if its
    /// `setup` errored or returned `Ok(false)`, or a hard dependency failed.
    pub failed: BTreeMap<String, String>,
}

impl SetupReport {
    /// Whether `domain` was successfully set up.
    #[must_use]
    pub fn is_set_up(&self, domain: &str) -> bool {
        self.set_up.iter().any(|d| d == domain)
    }
}

/// Port of `homeassistant.loader` — the integration registry + setup driver.
#[derive(Default)]
pub struct IntegrationLoader {
    integrations: BTreeMap<String, Box<dyn Integration>>,
}

impl IntegrationLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an integration under its manifest domain.
    ///
    /// # Errors
    /// [`LoaderError::DuplicateDomain`] if the domain is already registered.
    pub fn register(&mut self, integration: Box<dyn Integration>) -> Result<(), LoaderError> {
        let domain = integration.manifest().domain.clone();
        if self.integrations.contains_key(&domain) {
            return Err(LoaderError::DuplicateDomain(domain));
        }
        self.integrations.insert(domain, integration);
        Ok(())
    }

    /// Every registered domain (the domain registry), sorted.
    #[must_use]
    pub fn domains(&self) -> BTreeSet<String> {
        self.integrations.keys().cloned().collect()
    }

    /// The manifest for `domain`, if registered.
    #[must_use]
    pub fn manifest(&self, domain: &str) -> Option<&Manifest> {
        self.integrations.get(domain).map(|i| i.manifest())
    }

    /// Compute a setup order honouring hard `dependencies` (must precede their
    /// dependents) and soft `after_dependencies` (precede only when present).
    ///
    /// # Errors
    /// [`LoaderError::MissingDependency`] if a hard dependency is unregistered;
    /// [`LoaderError::DependencyCycle`] if the dependency graph has a cycle.
    pub fn setup_order(&self) -> Result<Vec<String>, LoaderError> {
        unimplemented!("RED")
    }

    /// Set up every registered integration in dependency order. An integration
    /// whose hard dependency failed (or was skipped) is itself failed without
    /// calling its `setup`.
    ///
    /// # Errors
    /// [`LoaderError`] from [`setup_order`](Self::setup_order) if the graph is
    /// invalid (nothing is set up in that case).
    pub fn setup_all(&self, ctx: &CoreContext) -> Result<SetupReport, LoaderError> {
        let _ = ctx;
        unimplemented!("RED")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::service::Service;
    use crate::state::{EntityId, StateAttributes};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A test integration that records the order it was set up in and can be
    /// told to fail.
    struct Recorder {
        manifest: Manifest,
        counter: Arc<AtomicUsize>,
        order: Arc<parking_lot::Mutex<Vec<(String, usize)>>>,
        fail: bool,
    }

    impl Integration for Recorder {
        fn manifest(&self) -> &Manifest {
            &self.manifest
        }
        fn setup(&self, ctx: &CoreContext) -> Result<bool, SetupError> {
            let n = self.counter.fetch_add(1, Ordering::SeqCst);
            self.order.lock().push((self.manifest.domain.clone(), n));
            if self.fail {
                return Err(SetupError::new(&self.manifest.domain, "boom"));
            }
            // prove the CoreContext seam: register a service named after the domain
            ctx.services
                .register(&self.manifest.domain, "noop", Service::new())
                .ok();
            Ok(true)
        }
    }

    fn recorder(
        manifest: Manifest,
        counter: &Arc<AtomicUsize>,
        order: &Arc<parking_lot::Mutex<Vec<(String, usize)>>>,
        fail: bool,
    ) -> Box<dyn Integration> {
        Box::new(Recorder {
            manifest,
            counter: counter.clone(),
            order: order.clone(),
            fail,
        })
    }

    fn harness() -> (Arc<AtomicUsize>, Arc<parking_lot::Mutex<Vec<(String, usize)>>>) {
        (Arc::new(AtomicUsize::new(0)), Arc::new(parking_lot::Mutex::new(Vec::new())))
    }

    #[test]
    fn register_rejects_duplicate_domain() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        loader.register(recorder(Manifest::new("hue", "Hue"), &c, &o, false)).expect("first");
        assert_eq!(
            loader.register(recorder(Manifest::new("hue", "Hue2"), &c, &o, false)).unwrap_err(),
            LoaderError::DuplicateDomain("hue".into())
        );
        assert_eq!(loader.domains(), BTreeSet::from(["hue".to_owned()]));
        assert_eq!(loader.manifest("hue").map(|m| m.name.clone()), Some("Hue".into()));
    }

    #[test]
    fn setup_order_respects_hard_dependencies() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        // light depends on hue; hue depends on network
        loader.register(recorder(Manifest::new("light", "Light").with_dependencies(&["hue"]), &c, &o, false)).expect("a");
        loader.register(recorder(Manifest::new("hue", "Hue").with_dependencies(&["network"]), &c, &o, false)).expect("b");
        loader.register(recorder(Manifest::new("network", "Net"), &c, &o, false)).expect("c");

        let order = loader.setup_order().expect("order");
        let pos = |d: &str| order.iter().position(|x| x == d).expect("present");
        assert!(pos("network") < pos("hue"));
        assert!(pos("hue") < pos("light"));
    }

    #[test]
    fn setup_order_after_dependency_only_orders_when_present() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        // recorder depends-after history; history is present → ordered
        loader.register(recorder(Manifest::new("recorder", "Rec").with_after_dependencies(&["history", "absent"]), &c, &o, false)).expect("a");
        loader.register(recorder(Manifest::new("history", "Hist"), &c, &o, false)).expect("b");
        let order = loader.setup_order().expect("order");
        let pos = |d: &str| order.iter().position(|x| x == d).expect("present");
        assert!(pos("history") < pos("recorder"));
        // "absent" is a soft dep that isn't registered — not an error
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn setup_order_missing_hard_dependency_errors() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        loader.register(recorder(Manifest::new("light", "Light").with_dependencies(&["nope"]), &c, &o, false)).expect("a");
        assert_eq!(
            loader.setup_order().unwrap_err(),
            LoaderError::MissingDependency { domain: "light".into(), dependency: "nope".into() }
        );
    }

    #[test]
    fn setup_order_detects_cycle() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        loader.register(recorder(Manifest::new("a", "A").with_dependencies(&["b"]), &c, &o, false)).expect("a");
        loader.register(recorder(Manifest::new("b", "B").with_dependencies(&["a"]), &c, &o, false)).expect("b");
        assert!(matches!(loader.setup_order().unwrap_err(), LoaderError::DependencyCycle(_)));
    }

    #[test]
    fn setup_all_runs_in_order_and_touches_core_context() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        loader.register(recorder(Manifest::new("hue", "Hue").with_dependencies(&["network"]), &c, &o, false)).expect("a");
        loader.register(recorder(Manifest::new("network", "Net"), &c, &o, false)).expect("b");

        let ctx = CoreContext::new();
        let report = loader.setup_all(&ctx).expect("report");
        assert!(report.is_set_up("network"));
        assert!(report.is_set_up("hue"));
        assert!(report.failed.is_empty());
        // network was set up before hue
        let order = o.lock();
        assert_eq!(order[0].0, "network");
        assert_eq!(order[1].0, "hue");
        // the CoreContext seam worked: both domains registered their service
        assert!(ctx.services.has_service("network", "noop"));
        assert!(ctx.services.has_service("hue", "noop"));
    }

    #[test]
    fn setup_all_skips_dependents_of_a_failed_integration() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        // network fails; hue depends on it and must be skipped (not called)
        loader.register(recorder(Manifest::new("network", "Net"), &c, &o, true)).expect("a");
        loader.register(recorder(Manifest::new("hue", "Hue").with_dependencies(&["network"]), &c, &o, false)).expect("b");

        let ctx = CoreContext::new();
        let report = loader.setup_all(&ctx).expect("report");
        assert!(!report.is_set_up("network"));
        assert!(!report.is_set_up("hue"));
        assert!(report.failed.contains_key("network"));
        assert!(report.failed.contains_key("hue"));
        // hue's setup was never called — only network ran
        assert_eq!(o.lock().len(), 1);
        assert_eq!(o.lock()[0].0, "network");
    }

    #[test]
    fn setup_all_continues_past_independent_failure() {
        let (c, o) = harness();
        let mut loader = IntegrationLoader::new();
        loader.register(recorder(Manifest::new("broken", "Broken"), &c, &o, true)).expect("a");
        loader.register(recorder(Manifest::new("fine", "Fine"), &c, &o, false)).expect("b");

        let ctx = CoreContext::new();
        let report = loader.setup_all(&ctx).expect("report");
        assert!(report.failed.contains_key("broken"));
        assert!(report.is_set_up("fine"));
        // unrelated entity work in fine's setup is observable
        ctx.states.set(
            EntityId::new("sensor", "x").expect("id"),
            "1",
            StateAttributes::new(),
            Context::new(),
        );
    }
}
