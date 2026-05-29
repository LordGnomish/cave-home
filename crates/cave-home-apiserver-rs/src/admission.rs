// SPDX-License-Identifier: Apache-2.0
//! Admission control: an ordered pipeline of mutating then validating plugins.
//!
//! Behavioural reference: Kubernetes docs "Admission Controllers" and the
//! documented admission phases — mutating admission runs first (and may rewrite
//! the object), then validating admission runs (and may only accept/reject).
//! This is a clean-room reimplementation of the documented two-phase contract.
//! Admission *webhooks* (the dynamic, out-of-process plugins) are deferred (see
//! `parity.manifest.toml`).

use crate::gvk::GroupVersionResource;
use crate::json::Value;
use crate::status::{Result, Status};

/// The operation that triggered admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Object creation.
    Create,
    /// Object update/replace/patch.
    Update,
    /// Object deletion.
    Delete,
}

/// The request an admission plugin sees.
#[derive(Clone, Debug)]
pub struct AdmissionRequest {
    /// Target resource.
    pub gvr: GroupVersionResource,
    /// Operation.
    pub operation: Operation,
    /// Namespace of the object (empty for cluster-scoped).
    pub namespace: String,
    /// The incoming object (None for delete).
    pub object: Option<Value>,
    /// The previous object (Some for update).
    pub old_object: Option<Value>,
}

/// A mutating plugin may rewrite `request.object`. Returning `Err` rejects.
pub trait MutatingPlugin {
    /// Plugin name (for diagnostics + deterministic ordering reporting).
    fn name(&self) -> &str;
    /// Mutate the request in place, or reject.
    ///
    /// # Errors
    /// A [`Status`] rejects the admission.
    fn admit(&self, request: &mut AdmissionRequest) -> Result<()>;
}

/// A validating plugin may only accept or reject; it must not mutate.
pub trait ValidatingPlugin {
    /// Plugin name.
    fn name(&self) -> &str;
    /// Validate the (already-mutated) request, or reject.
    ///
    /// # Errors
    /// A [`Status`] rejects the admission.
    fn validate(&self, request: &AdmissionRequest) -> Result<()>;
}

/// The admission chain: a fixed-order list of mutating plugins followed by a
/// fixed-order list of validating plugins.
#[derive(Default)]
pub struct AdmissionChain {
    mutating: Vec<Box<dyn MutatingPlugin>>,
    validating: Vec<Box<dyn ValidatingPlugin>>,
}

impl AdmissionChain {
    /// Empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a mutating plugin (executes in append order).
    #[must_use]
    pub fn with_mutating(mut self, p: Box<dyn MutatingPlugin>) -> Self {
        self.mutating.push(p);
        self
    }

    /// Append a validating plugin (executes in append order).
    #[must_use]
    pub fn with_validating(mut self, p: Box<dyn ValidatingPlugin>) -> Self {
        self.validating.push(p);
        self
    }

    /// Run the full pipeline: all mutating plugins in order (each sees the
    /// prior plugin's mutations), then all validating plugins in order. Returns
    /// the final, possibly-mutated request on success.
    ///
    /// # Errors
    /// The first plugin to reject short-circuits and its [`Status`] is returned.
    pub fn run(&self, mut request: AdmissionRequest) -> Result<AdmissionRequest> {
        for p in &self.mutating {
            p.admit(&mut request)?;
        }
        for p in &self.validating {
            p.validate(&request)?;
        }
        Ok(request)
    }

    /// The ordered names of mutating then validating plugins (for tests/audit).
    #[must_use]
    pub fn plugin_order(&self) -> Vec<String> {
        self.mutating
            .iter()
            .map(|p| p.name().to_string())
            .chain(self.validating.iter().map(|p| p.name().to_string()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Built-in plugins.
// ---------------------------------------------------------------------------

/// Mutating: fill server-defaulted metadata fields. Models the documented
/// behaviour that creates without an explicit namespace default to `"default"`
/// for namespaced resources.
pub struct DefaultFields {
    /// Whether the target resource is namespaced (caller-resolved from GVK).
    pub namespaced: bool,
}

impl MutatingPlugin for DefaultFields {
    fn name(&self) -> &str {
        "DefaultFields"
    }

    fn admit(&self, request: &mut AdmissionRequest) -> Result<()> {
        if request.operation != Operation::Create {
            return Ok(());
        }
        if let Some(object) = request.object.as_mut() {
            if object.as_object().is_none() {
                *object = Value::object();
            }
            if let Some(root) = object.as_object_mut() {
                let md = root.entry("metadata".to_string()).or_insert_with(Value::object);
                if md.as_object().is_none() {
                    *md = Value::object();
                }
                if let Some(m) = md.as_object_mut() {
                    if self.namespaced {
                        let needs_ns = m
                            .get("namespace")
                            .and_then(Value::as_str)
                            .map(str::is_empty)
                            .unwrap_or(true);
                        if needs_ns {
                            let ns = if request.namespace.is_empty() {
                                "default".to_string()
                            } else {
                                request.namespace.clone()
                            };
                            m.insert("namespace".into(), Value::from(ns.clone()));
                            request.namespace = ns;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Validating: reject objects targeting a namespace that does not exist, and
/// reject any mutation in a namespace that is terminating. Models the
/// documented NamespaceLifecycle admission controller. The set of live /
/// terminating namespaces is supplied by the caller (resolved from the
/// registry) to keep this plugin pure.
pub struct NamespaceExists {
    /// Namespaces that currently exist and are active.
    pub active: Vec<String>,
    /// Namespaces that exist but are terminating (mutations rejected).
    pub terminating: Vec<String>,
}

impl ValidatingPlugin for NamespaceExists {
    fn name(&self) -> &str {
        "NamespaceLifecycle"
    }

    fn validate(&self, request: &AdmissionRequest) -> Result<()> {
        // Cluster-scoped objects have no namespace to check.
        if request.namespace.is_empty() {
            return Ok(());
        }
        if self.terminating.iter().any(|n| n == &request.namespace) {
            return Err(Status::conflict(format!(
                "unable to create new content in namespace {} because it is being terminated",
                request.namespace
            )));
        }
        if !self.active.iter().any(|n| n == &request.namespace) {
            return Err(Status::not_found(format!(
                "namespace \"{}\" not found",
                request.namespace
            )));
        }
        Ok(())
    }
}

/// Validating: enforce that created/updated objects carry a non-empty
/// `metadata.name`. A small built-in that demonstrates rejection.
pub struct RequireName;

impl ValidatingPlugin for RequireName {
    fn name(&self) -> &str {
        "RequireName"
    }

    fn validate(&self, request: &AdmissionRequest) -> Result<()> {
        if request.operation == Operation::Delete {
            return Ok(());
        }
        let has_name = request
            .object
            .as_ref()
            .and_then(|o| o.pointer("metadata.name"))
            .and_then(Value::as_str)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if has_name {
            Ok(())
        } else {
            Err(Status::invalid("metadata.name is required"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::obj;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn req(op: Operation, ns: &str, object: Option<Value>) -> AdmissionRequest {
        AdmissionRequest {
            gvr: GroupVersionResource::new("", "v1", "pods"),
            operation: op,
            namespace: ns.to_string(),
            object,
            old_object: None,
        }
    }

    /// Order-recording mutating plugin for the ordering test.
    struct Recorder {
        tag: &'static str,
        log: Rc<RefCell<Vec<&'static str>>>,
    }
    impl MutatingPlugin for Recorder {
        fn name(&self) -> &str {
            self.tag
        }
        fn admit(&self, _r: &mut AdmissionRequest) -> Result<()> {
            self.log.borrow_mut().push(self.tag);
            Ok(())
        }
    }

    #[test]
    fn default_fields_sets_namespace_on_create() {
        let plugin = DefaultFields { namespaced: true };
        let mut r = req(Operation::Create, "", Some(obj([("metadata", obj([("name", Value::from("p"))]))])));
        plugin.admit(&mut r).expect("admit");
        let ns = r.object.as_ref().unwrap().pointer("metadata.namespace").and_then(Value::as_str);
        assert_eq!(ns, Some("default"));
        assert_eq!(r.namespace, "default");
    }

    #[test]
    fn default_fields_respects_request_namespace() {
        let plugin = DefaultFields { namespaced: true };
        let mut r = req(Operation::Create, "web", Some(obj([("metadata", obj([("name", Value::from("p"))]))])));
        plugin.admit(&mut r).expect("admit");
        let ns = r.object.as_ref().unwrap().pointer("metadata.namespace").and_then(Value::as_str);
        assert_eq!(ns, Some("web"));
    }

    #[test]
    fn namespace_exists_rejects_missing_namespace() {
        let plugin = NamespaceExists { active: vec!["default".into()], terminating: vec![] };
        let r = req(Operation::Create, "ghost", Some(Value::object()));
        let err = plugin.validate(&r).expect_err("reject");
        assert_eq!(err.reason, crate::status::StatusReason::NotFound);
    }

    #[test]
    fn namespace_exists_accepts_active_namespace() {
        let plugin = NamespaceExists { active: vec!["default".into()], terminating: vec![] };
        let r = req(Operation::Create, "default", Some(Value::object()));
        assert!(plugin.validate(&r).is_ok());
    }

    #[test]
    fn namespace_terminating_is_rejected() {
        let plugin = NamespaceExists { active: vec![], terminating: vec!["dying".into()] };
        let r = req(Operation::Create, "dying", Some(Value::object()));
        let err = plugin.validate(&r).expect_err("reject");
        assert_eq!(err.reason, crate::status::StatusReason::Conflict);
    }

    #[test]
    fn require_name_rejects_empty() {
        let plugin = RequireName;
        let r = req(Operation::Create, "default", Some(obj([("metadata", Value::object())])));
        assert!(plugin.validate(&r).is_err());
    }

    #[test]
    fn chain_runs_mutating_before_validating_in_order() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let chain = AdmissionChain::new()
            .with_mutating(Box::new(Recorder { tag: "m1", log: log.clone() }))
            .with_mutating(Box::new(Recorder { tag: "m2", log: log.clone() }))
            .with_validating(Box::new(RequireName));
        let r = req(Operation::Create, "default", Some(obj([("metadata", obj([("name", Value::from("p"))]))])));
        chain.run(r).expect("admit");
        assert_eq!(*log.borrow(), vec!["m1", "m2"]);
        assert_eq!(chain.plugin_order(), vec!["m1", "m2", "RequireName"]);
    }

    #[test]
    fn chain_short_circuits_on_validating_reject() {
        let chain = AdmissionChain::new()
            .with_mutating(Box::new(DefaultFields { namespaced: true }))
            .with_validating(Box::new(NamespaceExists {
                active: vec!["default".into()],
                terminating: vec![],
            }))
            .with_validating(Box::new(RequireName));
        // Create into a non-existent namespace -> NamespaceLifecycle rejects.
        let r = req(Operation::Create, "ghost", Some(obj([("metadata", obj([("name", Value::from("p"))]))])));
        let err = chain.run(r).expect_err("reject");
        assert_eq!(err.reason, crate::status::StatusReason::NotFound);
    }

    #[test]
    fn chain_mutation_is_visible_to_validation() {
        // DefaultFields sets namespace=default; NamespaceExists then sees it.
        let chain = AdmissionChain::new()
            .with_mutating(Box::new(DefaultFields { namespaced: true }))
            .with_validating(Box::new(NamespaceExists {
                active: vec!["default".into()],
                terminating: vec![],
            }));
        let r = req(Operation::Create, "", Some(obj([("metadata", obj([("name", Value::from("p"))]))])));
        let out = chain.run(r).expect("admit");
        assert_eq!(out.namespace, "default");
    }
}
