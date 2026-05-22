// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/namespace/namespace_controller.go
//         pkg/controller/namespace/deletion/namespaced_resources_deleter.go
//
//! NamespaceController.
//!
//! On namespace deletion: move to `Terminating`, cascade-delete every
//! namespaced resource the controller-manager knows about, then drop the
//! kubernetes.io/namespace-finalizer when no resources remain.

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::types::{
    DaemonSet, Deployment, Job, KubeResource, Namespace, NamespacePhase, NamespaceStatus,
    PersistentVolumeClaim, Pod, ReplicaSet, Secret, ServiceAccount, StatefulSet,
};

/// Finalizer added to every namespace at creation time. The controller
/// removes it once the cascade is done.
pub const NAMESPACE_FINALIZER: &str = "kubernetes";

/// Mirrors `NamespaceController.syncNamespace`.
pub async fn sync_namespace<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
) -> ApiResult<NamespacePhase> {
    let ns_obj: Namespace = client.get(None, namespace).await?;
    // Not being deleted? Nothing to do.
    if ns_obj.meta().deletion_timestamp_ms.is_none() {
        return Ok(ns_obj.status.phase);
    }
    // Flip to Terminating if not already.
    let mut ns_obj = if ns_obj.status.phase != NamespacePhase::Terminating {
        let mut ns2 = ns_obj.clone();
        ns2.status = NamespaceStatus {
            phase: NamespacePhase::Terminating,
        };
        client.update(None, ns2).await?
    } else {
        ns_obj
    };

    // Cascade-delete every known namespaced kind.
    let _ = delete_all::<Pod, _>(client, namespace).await?;
    let _ = delete_all::<ReplicaSet, _>(client, namespace).await?;
    let _ = delete_all::<Deployment, _>(client, namespace).await?;
    let _ = delete_all::<DaemonSet, _>(client, namespace).await?;
    let _ = delete_all::<StatefulSet, _>(client, namespace).await?;
    let _ = delete_all::<Job, _>(client, namespace).await?;
    let _ = delete_all::<Secret, _>(client, namespace).await?;
    let _ = delete_all::<ServiceAccount, _>(client, namespace).await?;
    let _ = delete_all::<PersistentVolumeClaim, _>(client, namespace).await?;

    // Drop the well-known finalizer + (in upstream) actually delete the
    // namespace object. The InMemoryApiClient honours `delete()` directly.
    let was_only_finalizer = ns_obj.meta().finalizers.len() == 1
        && ns_obj.meta().finalizers[0] == NAMESPACE_FINALIZER;
    ns_obj
        .meta_mut()
        .finalizers
        .retain(|f| f != NAMESPACE_FINALIZER);
    if was_only_finalizer {
        client.delete("Namespace", None, namespace).await?;
    } else {
        client.update(None, ns_obj).await?;
    }
    Ok(NamespacePhase::Terminating)
}

async fn delete_all<R: KubeResource, C: ControllerApiClient>(
    client: &C,
    namespace: &str,
) -> ApiResult<usize> {
    let objs: Vec<R> = client.list(Some(namespace), None).await?;
    let n = objs.len();
    for obj in objs {
        client.delete(R::kind(), Some(namespace), obj.name()).await?;
    }
    Ok(n)
}

pub struct NamespaceController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> NamespaceController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile(&self, namespace: &str) -> ApiResult<()> {
        sync_namespace(self.client.as_ref(), namespace).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::ObjectMeta;

    fn make_terminating_ns(name: &str) -> Namespace {
        Namespace {
            metadata: ObjectMeta {
                name: name.into(),
                deletion_timestamp_ms: Some(1),
                finalizers: vec![NAMESPACE_FINALIZER.into()],
                ..Default::default()
            },
            status: NamespaceStatus {
                phase: NamespacePhase::Active,
            },
        }
    }

    #[tokio::test]
    async fn no_op_when_not_deleted() {
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
        let phase = sync_namespace(&c, "default").await.unwrap();
        assert_eq!(phase, NamespacePhase::Active);
        assert_eq!(c.count("Namespace"), 1);
    }

    #[tokio::test]
    async fn cascade_deletes_pods_in_terminating_namespace() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_terminating_ns("dying"));
        c.seed(
            Some("dying"),
            Pod {
                metadata: ObjectMeta {
                    name: "p1".into(),
                    namespace: "dying".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sync_namespace(&c, "dying").await.unwrap();
        assert_eq!(c.count("Pod"), 0);
    }

    #[tokio::test]
    async fn cascade_deletes_replicasets_and_deployments() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_terminating_ns("dying"));
        c.seed(
            Some("dying"),
            ReplicaSet {
                metadata: ObjectMeta {
                    name: "rs1".into(),
                    namespace: "dying".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        c.seed(
            Some("dying"),
            Deployment {
                metadata: ObjectMeta {
                    name: "d1".into(),
                    namespace: "dying".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sync_namespace(&c, "dying").await.unwrap();
        assert_eq!(c.count("ReplicaSet"), 0);
        assert_eq!(c.count("Deployment"), 0);
    }

    #[tokio::test]
    async fn finalizer_removal_deletes_namespace_object() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_terminating_ns("dying"));
        sync_namespace(&c, "dying").await.unwrap();
        assert_eq!(c.count("Namespace"), 0);
    }

    #[tokio::test]
    async fn additional_finalizers_keep_namespace_alive() {
        let c = InMemoryApiClient::new();
        let mut ns = make_terminating_ns("dying");
        ns.metadata.finalizers.push("user.example.com/wait".into());
        c.seed(None, ns);
        sync_namespace(&c, "dying").await.unwrap();
        // The kubernetes finalizer is dropped but the user one stays.
        let live = c.get::<Namespace>(None, "dying").await.unwrap();
        assert_eq!(live.metadata.finalizers, vec!["user.example.com/wait"]);
    }
}
