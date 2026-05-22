// SPDX-License-Identifier: Apache-2.0
//! Admission control chain — RED phase scaffold.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/apiserver/pkg/admission/{interfaces.go,chain.go}
//! - plugin/pkg/admission/namespace/lifecycle/
//! - plugin/pkg/admission/serviceaccount/

use async_trait::async_trait;
use thiserror::Error;

use crate::types::AdmissionAttributes;

pub mod namespace_lifecycle;
pub mod service_account;

pub use namespace_lifecycle::NamespaceLifecycle;
pub use service_account::ServiceAccount;

/// Admission error variants.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AdmissionError {
    /// Validation failure — surfaced to client as 422.
    #[error("admission rejected: {0}")]
    Rejected(String),
    /// Internal failure — surfaced as 500.
    #[error("internal admission failure: {0}")]
    Internal(String),
}

/// Convenience alias.
pub type AdmissionResult = Result<(), AdmissionError>;

/// Mutating + validating admission controller. The contract is the same as
/// upstream: `admit` may mutate `attrs.object` in place; `validate` is
/// read-only and runs after every mutating step.
///
/// Source: staging/src/k8s.io/apiserver/pkg/admission/interfaces.go::{MutationInterface,ValidationInterface}
#[async_trait]
pub trait AdmissionController: Send + Sync {
    /// Mutating phase. Default: no-op.
    async fn admit(&self, _attrs: &mut AdmissionAttributes) -> AdmissionResult {
        Ok(())
    }
    /// Validating phase. Default: no-op.
    async fn validate(&self, _attrs: &AdmissionAttributes) -> AdmissionResult {
        Ok(())
    }
    /// Human-readable name (matches the upstream plugin name).
    fn name(&self) -> &str;
}

/// Linear chain executor.
///
/// Source: staging/src/k8s.io/apiserver/pkg/admission/chain.go::chainAdmissionHandler
pub struct AdmissionChain {
    controllers: Vec<Box<dyn AdmissionController>>,
}

impl AdmissionChain {
    /// Construct an empty chain.
    #[must_use]
    pub fn new(controllers: Vec<Box<dyn AdmissionController>>) -> Self {
        Self { controllers }
    }

    /// Names of all installed controllers, in order.
    #[must_use]
    pub fn controller_names(&self) -> Vec<&str> {
        self.controllers.iter().map(|c| c.name()).collect()
    }
}

#[async_trait]
impl AdmissionController for AdmissionChain {
    async fn admit(&self, attrs: &mut AdmissionAttributes) -> AdmissionResult {
        for c in &self.controllers {
            c.admit(attrs).await?;
        }
        Ok(())
    }
    async fn validate(&self, attrs: &AdmissionAttributes) -> AdmissionResult {
        for c in &self.controllers {
            c.validate(attrs).await?;
        }
        Ok(())
    }
    fn name(&self) -> &str {
        "AdmissionChain"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ResourceRef, UserInfo, Verb};

    #[tokio::test]
    async fn empty_chain_is_a_no_op() {
        let chain = AdmissionChain::new(vec![]);
        let mut attrs = AdmissionAttributes {
            resource: ResourceRef::namespaced("", "v1", "pods", "default", "p"),
            verb: Verb::Create,
            user: UserInfo::new("alice"),
            object: None,
            old_object: None,
            dry_run: false,
        };
        chain.admit(&mut attrs).await.expect("admit");
        chain.validate(&attrs).await.expect("validate");
    }

    #[tokio::test]
    async fn chain_reports_its_controllers_in_order() {
        let chain = AdmissionChain::new(vec![
            Box::new(NamespaceLifecycle::new(vec!["default".into()])),
            Box::new(ServiceAccount::new()),
        ]);
        assert_eq!(
            chain.controller_names(),
            vec!["NamespaceLifecycle", "ServiceAccount"]
        );
    }
}
