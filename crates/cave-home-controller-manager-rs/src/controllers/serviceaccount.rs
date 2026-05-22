// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/serviceaccount/serviceaccounts_controller.go
//         pkg/controller/serviceaccount/tokens_controller.go
//
//! ServiceAccountController + TokenController.
//!
//! * `ServiceAccountController` — ensures every active namespace has a
//!   `default` ServiceAccount.
//! * `TokenController` — ensures every ServiceAccount has a matching token
//!   secret with type `kubernetes.io/service-account-token`.
//!
//! Phase 2 generates an opaque, deterministic token payload (not a JWT —
//! signed JWT generation belongs in `cave-home-apiserver-rs`).

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::types::{
    new_controller_ref, KubeResource, Namespace, NamespacePhase, ObjectMeta, ObjectReference,
    Secret, ServiceAccount,
};

/// Name of the auto-created default ServiceAccount.
pub const DEFAULT_SA_NAME: &str = "default";

/// `Secret.Type` for SA tokens.
pub const SA_TOKEN_SECRET_TYPE: &str = "kubernetes.io/service-account-token";

/// `Secret.Data` key for the token payload.
pub const SA_TOKEN_DATA_KEY: &str = "token";

// ---------------------------------------------------------------------------
// ServiceAccountController.sync_namespace
// ---------------------------------------------------------------------------

/// Ensure the `default` SA exists in `namespace`.
///
/// Mirrors `ServiceAccountsController.syncServiceAccount` (well, the
/// namespace-driven half).
pub async fn ensure_default_service_account<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
) -> ApiResult<ServiceAccount> {
    // Don't auto-create SAs in terminating namespaces.
    if let Ok(ns) = client.get::<Namespace>(None, namespace).await {
        if ns.status.phase == NamespacePhase::Terminating {
            return Err(crate::api_client::ApiError::Invalid(format!(
                "namespace {namespace} is terminating"
            )));
        }
    }
    if let Ok(existing) = client.get::<ServiceAccount>(Some(namespace), DEFAULT_SA_NAME).await {
        return Ok(existing);
    }
    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: DEFAULT_SA_NAME.into(),
            namespace: namespace.into(),
            ..Default::default()
        },
        secrets: Vec::new(),
    };
    client.create(Some(namespace), sa).await
}

// ---------------------------------------------------------------------------
// TokenController.sync_service_account
// ---------------------------------------------------------------------------

/// Ensure `sa` has a matching token Secret.
///
/// Mirrors `TokensController.syncServiceAccount`.
pub async fn ensure_token_for<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    sa_name: &str,
) -> ApiResult<Secret> {
    let mut sa: ServiceAccount = client.get(Some(namespace), sa_name).await?;
    // Already has a token reference?
    if let Some(secret_ref) = sa.secrets.iter().find(|s| s.kind == "Secret") {
        if let Ok(s) = client
            .get::<Secret>(Some(namespace), &secret_ref.name)
            .await
        {
            if s.secret_type == SA_TOKEN_SECRET_TYPE {
                return Ok(s);
            }
        }
    }
    let token_name = format!("{sa_name}-token");
    let mut data = std::collections::BTreeMap::new();
    data.insert(
        SA_TOKEN_DATA_KEY.into(),
        // Opaque, deterministic payload — real JWT signing is
        // `cave-home-apiserver-rs`'s responsibility.
        format!("token-for-{namespace}-{sa_name}").into_bytes(),
    );
    let secret = Secret {
        metadata: ObjectMeta {
            name: token_name.clone(),
            namespace: namespace.into(),
            owner_references: vec![new_controller_ref(&sa, "v1")],
            ..Default::default()
        },
        data,
        secret_type: SA_TOKEN_SECRET_TYPE.into(),
    };
    let created = client.create(Some(namespace), secret).await?;
    sa.secrets.push(ObjectReference {
        kind: "Secret".into(),
        namespace: namespace.into(),
        name: token_name,
        uid: created.uid().clone(),
    });
    client.update(Some(namespace), sa).await?;
    Ok(created)
}

// ---------------------------------------------------------------------------
// Controller handles
// ---------------------------------------------------------------------------

pub struct ServiceAccountController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> ServiceAccountController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile_namespace(&self, namespace: &str) -> ApiResult<()> {
        ensure_default_service_account(self.client.as_ref(), namespace).await?;
        Ok(())
    }
}

pub struct TokenController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> TokenController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        ensure_token_for(self.client.as_ref(), &ns, &name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;

    #[tokio::test]
    async fn creates_default_sa_when_missing() {
        let c = InMemoryApiClient::new();
        c.seed(
            None,
            Namespace {
                metadata: ObjectMeta {
                    name: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        ensure_default_service_account(&c, "default").await.unwrap();
        let sa = c.get::<ServiceAccount>(Some("default"), DEFAULT_SA_NAME).await.unwrap();
        assert_eq!(sa.name(), DEFAULT_SA_NAME);
    }

    #[tokio::test]
    async fn idempotent_when_sa_already_exists() {
        let c = InMemoryApiClient::new();
        c.seed(
            None,
            Namespace {
                metadata: ObjectMeta {
                    name: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        ensure_default_service_account(&c, "default").await.unwrap();
        ensure_default_service_account(&c, "default").await.unwrap();
        assert_eq!(c.count("ServiceAccount"), 1);
    }

    #[tokio::test]
    async fn does_not_create_sa_in_terminating_ns() {
        let c = InMemoryApiClient::new();
        c.seed(
            None,
            Namespace {
                metadata: ObjectMeta {
                    name: "dying".into(),
                    ..Default::default()
                },
                status: crate::types::NamespaceStatus {
                    phase: NamespacePhase::Terminating,
                },
            },
        );
        assert!(ensure_default_service_account(&c, "dying").await.is_err());
        assert_eq!(c.count("ServiceAccount"), 0);
    }

    #[tokio::test]
    async fn creates_token_for_sa() {
        let c = InMemoryApiClient::new();
        c.seed(
            None,
            Namespace {
                metadata: ObjectMeta {
                    name: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let _sa = ensure_default_service_account(&c, "default").await.unwrap();
        let tok = ensure_token_for(&c, "default", DEFAULT_SA_NAME).await.unwrap();
        assert_eq!(tok.secret_type, SA_TOKEN_SECRET_TYPE);
        assert!(tok.data.contains_key(SA_TOKEN_DATA_KEY));
    }

    #[tokio::test]
    async fn token_creation_is_idempotent() {
        let c = InMemoryApiClient::new();
        c.seed(
            None,
            Namespace {
                metadata: ObjectMeta {
                    name: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let _sa = ensure_default_service_account(&c, "default").await.unwrap();
        ensure_token_for(&c, "default", DEFAULT_SA_NAME).await.unwrap();
        ensure_token_for(&c, "default", DEFAULT_SA_NAME).await.unwrap();
        assert_eq!(c.count("Secret"), 1);
    }

    #[tokio::test]
    async fn sa_secrets_list_references_token() {
        let c = InMemoryApiClient::new();
        c.seed(
            None,
            Namespace {
                metadata: ObjectMeta {
                    name: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        ensure_default_service_account(&c, "default").await.unwrap();
        ensure_token_for(&c, "default", DEFAULT_SA_NAME).await.unwrap();
        let sa = c
            .get::<ServiceAccount>(Some("default"), DEFAULT_SA_NAME)
            .await
            .unwrap();
        assert_eq!(sa.secrets.len(), 1);
        assert_eq!(sa.secrets[0].kind, "Secret");
    }
}
