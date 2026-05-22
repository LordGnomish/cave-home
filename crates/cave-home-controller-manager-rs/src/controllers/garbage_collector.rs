// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/garbagecollector/garbagecollector.go
//         pkg/controller/garbagecollector/graph.go
//         pkg/controller/garbagecollector/graph_builder.go
//
//! GarbageCollector.
//!
//! When an owner is deleted with the default `Background` propagation policy,
//! every dependent (an object whose `metadata.ownerReferences` points back
//! at the owner) is queued for delete. The Phase 2 port supports the
//! `Background` propagation policy only; `Foreground` and `Orphan` are
//! recorded as `[[unmapped]]`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::types::{
    DaemonSet, Deployment, Job, KubeResource, ObjectMeta, PersistentVolumeClaim, Pod, ReplicaSet,
    StatefulSet, Uid,
};

/// A vertex in the owner graph — `(kind, namespace, name, uid)`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObjectRef {
    pub kind: String,
    pub namespace: String,
    pub name: String,
    pub uid: Uid,
}

/// In-memory owner graph — built by [`build_graph`] from the current cluster
/// state. Used by [`cascade_delete`] to walk dependents.
#[derive(Default)]
pub struct OwnerGraph {
    /// `uid -> ObjectRef` for every observed object.
    pub by_uid: HashMap<Uid, ObjectRef>,
    /// `owner_uid -> [dependent_ref]`.
    pub dependents: HashMap<Uid, Vec<ObjectRef>>,
}

impl OwnerGraph {
    fn add(&mut self, obj: ObjectRef, owners: &[Uid]) {
        for owner_uid in owners {
            self.dependents
                .entry(owner_uid.clone())
                .or_default()
                .push(obj.clone());
        }
        self.by_uid.insert(obj.uid.clone(), obj);
    }
}

fn meta_owners(meta: &ObjectMeta) -> Vec<Uid> {
    meta.owner_references
        .iter()
        .filter(|o| o.controller)
        .map(|o| o.uid.clone())
        .collect()
}

fn obj_ref_of<R: KubeResource>(r: &R) -> ObjectRef {
    ObjectRef {
        kind: R::kind().to_string(),
        namespace: r.namespace().to_string(),
        name: r.name().to_string(),
        uid: r.uid().clone(),
    }
}

async fn collect<R: KubeResource, C: ControllerApiClient>(
    client: &C,
    graph: &mut OwnerGraph,
) -> ApiResult<()> {
    let objs: Vec<R> = client.list(None, None).await?;
    for obj in objs {
        let owners = meta_owners(obj.meta());
        graph.add(obj_ref_of(&obj), &owners);
    }
    Ok(())
}

/// Build a fresh owner graph for every kind known to the Phase 2 controllers.
pub async fn build_graph<C: ControllerApiClient>(client: &C) -> ApiResult<OwnerGraph> {
    let mut g = OwnerGraph::default();
    collect::<Pod, _>(client, &mut g).await?;
    collect::<ReplicaSet, _>(client, &mut g).await?;
    collect::<Deployment, _>(client, &mut g).await?;
    collect::<DaemonSet, _>(client, &mut g).await?;
    collect::<StatefulSet, _>(client, &mut g).await?;
    collect::<Job, _>(client, &mut g).await?;
    collect::<PersistentVolumeClaim, _>(client, &mut g).await?;
    Ok(g)
}

/// Delete `(kind, namespace, name)` and recursively delete every dependent.
///
/// Mirrors `GarbageCollector.attemptToDeleteItem` with `PropagationPolicy =
/// Background`.
pub async fn cascade_delete<C: ControllerApiClient>(
    client: &C,
    root_uid: &Uid,
    root_kind: &str,
    root_namespace: &str,
    root_name: &str,
) -> ApiResult<usize> {
    let graph = build_graph(client).await?;
    let mut to_delete: Vec<ObjectRef> = Vec::new();
    let mut seen: HashSet<Uid> = HashSet::new();
    let mut stack: Vec<Uid> = vec![root_uid.clone()];
    while let Some(uid) = stack.pop() {
        if !seen.insert(uid.clone()) {
            continue;
        }
        if let Some(deps) = graph.dependents.get(&uid) {
            for d in deps {
                to_delete.push(d.clone());
                stack.push(d.uid.clone());
            }
        }
    }
    // Delete deepest first to avoid orphan windows.
    to_delete.reverse();
    let mut count = 0;
    for obj in &to_delete {
        if client
            .delete(&obj.kind, Some(&obj.namespace), &obj.name)
            .await
            .is_ok()
        {
            count += 1;
        }
    }
    // Delete the root last.
    let ns = if root_namespace.is_empty() {
        None
    } else {
        Some(root_namespace)
    };
    if client.delete(root_kind, ns, root_name).await.is_ok() {
        count += 1;
    }
    Ok(count)
}

/// Orphan-cleanup sweep: for every observed dependent, if its sole controller
/// owner is gone, delete it.
///
/// Mirrors `GarbageCollector.runProcessGraphChanges` orphan-detection path.
pub async fn sweep_orphans<C: ControllerApiClient>(client: &C) -> ApiResult<usize> {
    let graph = build_graph(client).await?;
    let mut deleted = 0;
    for (uid, obj) in &graph.by_uid {
        let _ = uid;
        // We need the per-object owner_references to test "controller-owner
        // missing". Re-fetch the type-specific kind.
        let is_orphan = is_orphaned(client, obj).await?;
        if is_orphan {
            if client
                .delete(&obj.kind, Some(&obj.namespace), &obj.name)
                .await
                .is_ok()
            {
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

async fn is_orphaned<C: ControllerApiClient>(client: &C, obj: &ObjectRef) -> ApiResult<bool> {
    let meta = match obj.kind.as_str() {
        "Pod" => client
            .get::<Pod>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        "ReplicaSet" => client
            .get::<ReplicaSet>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        "Deployment" => client
            .get::<Deployment>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        "DaemonSet" => client
            .get::<DaemonSet>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        "StatefulSet" => client
            .get::<StatefulSet>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        "Job" => client
            .get::<Job>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        "PersistentVolumeClaim" => client
            .get::<PersistentVolumeClaim>(Some(&obj.namespace), &obj.name)
            .await
            .ok()
            .map(|o| o.metadata),
        _ => None,
    };
    let Some(meta) = meta else {
        return Ok(false);
    };
    let controllers: Vec<&Uid> = meta
        .owner_references
        .iter()
        .filter(|r| r.controller)
        .map(|r| &r.uid)
        .collect();
    if controllers.is_empty() {
        return Ok(false);
    }
    // Build a quick set of live UIDs across known kinds. (Reusing build_graph
    // would be cheaper but it's already invoked one level up.)
    let graph = build_graph(client).await?;
    let any_alive = controllers.iter().any(|u| graph.by_uid.contains_key(*u));
    Ok(!any_alive)
}

pub struct GarbageCollector<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> GarbageCollector<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn cascade_delete(
        &self,
        root_uid: &Uid,
        root_kind: &str,
        root_namespace: &str,
        root_name: &str,
    ) -> ApiResult<usize> {
        cascade_delete(self.client.as_ref(), root_uid, root_kind, root_namespace, root_name).await
    }

    pub async fn sweep_orphans(&self) -> ApiResult<usize> {
        sweep_orphans(self.client.as_ref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{new_controller_ref, ObjectMeta, PodSpec, PodStatus, ReplicaSetSpec};

    async fn seed_rs_with_pods(c: &InMemoryApiClient) -> (Uid, Vec<String>) {
        let rs = c.seed(
            Some("default"),
            ReplicaSet {
                metadata: ObjectMeta {
                    name: "rs1".into(),
                    namespace: "default".into(),
                    ..Default::default()
                },
                spec: ReplicaSetSpec::default(),
                ..Default::default()
            },
        );
        let mut pods = Vec::new();
        for i in 0..3 {
            let p = c.seed(
                Some("default"),
                Pod {
                    metadata: ObjectMeta {
                        name: format!("rs1-{i}"),
                        namespace: "default".into(),
                        owner_references: vec![new_controller_ref(&rs, "apps/v1")],
                        ..Default::default()
                    },
                    spec: PodSpec::default(),
                    status: PodStatus::default(),
                },
            );
            pods.push(p.name().to_string());
        }
        (rs.uid().clone(), pods)
    }

    #[tokio::test]
    async fn build_graph_resolves_dependents() {
        let c = InMemoryApiClient::new();
        let (uid, _pods) = seed_rs_with_pods(&c).await;
        let g = build_graph(&c).await.unwrap();
        let deps = g.dependents.get(&uid).unwrap();
        assert_eq!(deps.len(), 3);
    }

    #[tokio::test]
    async fn cascade_delete_removes_root_and_dependents() {
        let c = InMemoryApiClient::new();
        let (uid, _pods) = seed_rs_with_pods(&c).await;
        let n = cascade_delete(&c, &uid, "ReplicaSet", "default", "rs1").await.unwrap();
        assert_eq!(n, 4);
        assert_eq!(c.count("Pod"), 0);
        assert_eq!(c.count("ReplicaSet"), 0);
    }

    #[tokio::test]
    async fn cascade_delete_walks_deployment_to_pods() {
        let c = InMemoryApiClient::new();
        let d = c.seed(
            Some("default"),
            Deployment {
                metadata: ObjectMeta {
                    name: "d1".into(),
                    namespace: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let rs = c.seed(
            Some("default"),
            ReplicaSet {
                metadata: ObjectMeta {
                    name: "rs1".into(),
                    namespace: "default".into(),
                    owner_references: vec![new_controller_ref(&d, "apps/v1")],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        for i in 0..2 {
            c.seed(
                Some("default"),
                Pod {
                    metadata: ObjectMeta {
                        name: format!("rs1-{i}"),
                        namespace: "default".into(),
                        owner_references: vec![new_controller_ref(&rs, "apps/v1")],
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        }
        let n = cascade_delete(&c, d.uid(), "Deployment", "default", "d1")
            .await
            .unwrap();
        assert_eq!(n, 4);
        assert_eq!(c.count("Pod"), 0);
        assert_eq!(c.count("ReplicaSet"), 0);
        assert_eq!(c.count("Deployment"), 0);
    }

    #[tokio::test]
    async fn sweep_orphans_deletes_pod_whose_owner_is_gone() {
        let c = InMemoryApiClient::new();
        // Stamp a pod with an owner-ref whose UID doesn't correspond to any
        // live resource.
        c.seed(
            Some("default"),
            Pod {
                metadata: ObjectMeta {
                    name: "orphan".into(),
                    namespace: "default".into(),
                    owner_references: vec![crate::types::OwnerReference {
                        api_version: "apps/v1".into(),
                        kind: "ReplicaSet".into(),
                        name: "missing".into(),
                        uid: Uid::new("dead-uid"),
                        controller: true,
                        block_owner_deletion: true,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let n = sweep_orphans(&c).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(c.count("Pod"), 0);
    }

    #[tokio::test]
    async fn sweep_orphans_keeps_pods_whose_owner_is_alive() {
        let c = InMemoryApiClient::new();
        let (_uid, _pods) = seed_rs_with_pods(&c).await;
        let n = sweep_orphans(&c).await.unwrap();
        assert_eq!(n, 0);
        assert_eq!(c.count("Pod"), 3);
    }
}
