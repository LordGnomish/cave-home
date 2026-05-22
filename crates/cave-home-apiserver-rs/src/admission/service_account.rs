// SPDX-License-Identifier: Apache-2.0
//! ServiceAccount admission controller — RED phase scaffold.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! plugin/pkg/admission/serviceaccount/admission.go::serviceAccount

use async_trait::async_trait;

use crate::admission::{AdmissionController, AdmissionError, AdmissionResult};
use crate::types::{AdmissionAttributes, Verb};

/// Mutates incoming Pods to set `spec.serviceAccountName = "default"` when
/// the field is unset, and validates that the referenced SA exists.
///
/// Phase 2 simplification: SA existence is a no-op (controller-manager
/// auto-creates a `default` SA in every namespace, so this is the same
/// invariant). Token volume projection is Phase 2b.
pub struct ServiceAccount;

impl ServiceAccount {
    /// Construct a controller.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ServiceAccount {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AdmissionController for ServiceAccount {
    async fn admit(&self, attrs: &mut AdmissionAttributes) -> AdmissionResult {
        // Only mutate on Pod CREATE / UPDATE.
        if attrs.resource.resource != "pods" {
            return Ok(());
        }
        if !matches!(attrs.verb, Verb::Create | Verb::Update) {
            return Ok(());
        }
        let Some(obj) = attrs.object.as_mut() else {
            return Ok(());
        };
        let spec = obj
            .get_mut("spec")
            .ok_or_else(|| AdmissionError::Rejected("pod missing spec".into()))?;
        let map = spec
            .as_object_mut()
            .ok_or_else(|| AdmissionError::Rejected("pod spec is not an object".into()))?;
        let needs_default = map
            .get("serviceAccountName")
            .and_then(|v| v.as_str())
            .map(str::is_empty)
            .unwrap_or(true);
        if needs_default {
            map.insert(
                "serviceAccountName".to_string(),
                serde_json::Value::String("default".into()),
            );
        }
        Ok(())
    }

    async fn validate(&self, attrs: &AdmissionAttributes) -> AdmissionResult {
        // Phase 2: a Pod's serviceAccountName must be a non-empty string.
        if attrs.resource.resource != "pods" {
            return Ok(());
        }
        if !matches!(attrs.verb, Verb::Create | Verb::Update) {
            return Ok(());
        }
        let Some(obj) = attrs.object.as_ref() else {
            return Ok(());
        };
        let sa = obj
            .get("spec")
            .and_then(|s| s.get("serviceAccountName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if sa.is_empty() {
            return Err(AdmissionError::Rejected(
                "pod.spec.serviceAccountName must be non-empty".into(),
            ));
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "ServiceAccount"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ResourceRef, UserInfo};

    fn pod_attrs(verb: Verb, sa: Option<&str>) -> AdmissionAttributes {
        let spec = match sa {
            Some(name) => serde_json::json!({ "serviceAccountName": name }),
            None => serde_json::json!({}),
        };
        AdmissionAttributes {
            resource: ResourceRef::namespaced("", "v1", "pods", "default", "p"),
            verb,
            user: UserInfo::new("alice"),
            object: Some(serde_json::json!({"spec": spec})),
            old_object: None,
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn admit_sets_default_sa_when_unset() {
        let c = ServiceAccount::new();
        let mut a = pod_attrs(Verb::Create, None);
        c.admit(&mut a).await.expect("admit");
        let sa = a
            .object
            .as_ref()
            .and_then(|o| o.get("spec"))
            .and_then(|s| s.get("serviceAccountName"))
            .and_then(|v| v.as_str())
            .expect("sa set");
        assert_eq!(sa, "default");
    }

    #[tokio::test]
    async fn admit_preserves_user_supplied_sa() {
        let c = ServiceAccount::new();
        let mut a = pod_attrs(Verb::Create, Some("my-sa"));
        c.admit(&mut a).await.expect("admit");
        let sa = a
            .object
            .as_ref()
            .and_then(|o| o.get("spec"))
            .and_then(|s| s.get("serviceAccountName"))
            .and_then(|v| v.as_str())
            .expect("sa set");
        assert_eq!(sa, "my-sa");
    }

    #[tokio::test]
    async fn validate_rejects_empty_sa() {
        let c = ServiceAccount::new();
        let a = pod_attrs(Verb::Create, Some(""));
        let res = c.validate(&a).await;
        assert!(matches!(res, Err(AdmissionError::Rejected(_))));
    }

    #[tokio::test]
    async fn ignores_non_pod_resources() {
        let c = ServiceAccount::new();
        let mut a = AdmissionAttributes {
            resource: ResourceRef::namespaced("", "v1", "configmaps", "default", "cm"),
            verb: Verb::Create,
            user: UserInfo::new("alice"),
            object: Some(serde_json::json!({"data": {"a": "b"}})),
            old_object: None,
            dry_run: false,
        };
        c.admit(&mut a).await.expect("admit");
        c.validate(&a).await.expect("validate");
    }
}
