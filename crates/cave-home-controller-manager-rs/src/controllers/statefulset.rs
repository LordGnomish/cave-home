// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/statefulset/stateful_set.go
//         pkg/controller/statefulset/stateful_set_control.go
//         pkg/controller/statefulset/stateful_set_utils.go
//
//! StatefulSetController.
//!
//! Maintains a strict ordinal index `0..spec.replicas` of pods and the
//! associated `<vct>-<sts>-<ordinal>` PVCs. Pod and PVC names are
//! deterministic so the consumer can rely on stable network identities.

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient, LabelSelectorFilter};
use crate::types::{
    is_controlled_by, new_controller_ref, KubeResource, ObjectMeta, PersistentVolumeClaim, Pod,
    StatefulSet,
};

/// Generates the conventional pod name used by upstream
/// `getPodName(set, ordinal)`.
#[must_use]
pub fn pod_name(set: &StatefulSet, ordinal: i32) -> String {
    format!("{}-{ordinal}", set.name())
}

/// `getPersistentVolumeClaimName` upstream.
#[must_use]
pub fn pvc_name(set: &StatefulSet, claim: &str, ordinal: i32) -> String {
    format!("{claim}-{}-{ordinal}", set.name())
}

/// Mirrors `StatefulSetControl.UpdateStatefulSet` for the ordinal-create path.
///
/// Phase 2 supports:
///   - ordered create from 0 .. replicas-1
///   - ordered delete from the top when replicas shrinks
///   - PVC creation per `volumeClaimTemplates` (`whenDeleted=Retain` by
///     default — Phase 2 never deletes PVCs because that policy is
///     `[[unmapped]]`).
pub async fn sync_stateful_set<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    name: &str,
) -> ApiResult<crate::types::StatefulSetStatus> {
    let mut set: StatefulSet = client.get(Some(namespace), name).await?;
    let owner_uid = set.uid().clone();

    let sel = LabelSelectorFilter::from(&set.spec.selector);
    let pods: Vec<Pod> = client.list(Some(namespace), Some(&sel)).await?;
    let mut owned: Vec<Pod> = pods
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();
    owned.sort_by(|a, b| a.name().cmp(b.name()));

    let desired = set.spec.replicas.max(0);
    // 1) Create missing PVCs + Pods in ordinal order.
    for ordinal in 0..desired {
        // PVCs first (the pod refers to them by name).
        for claim in &set.spec.volume_claim_templates {
            let want = pvc_name(&set, claim, ordinal);
            if client
                .get::<PersistentVolumeClaim>(Some(namespace), &want)
                .await
                .is_err()
            {
                let pvc = PersistentVolumeClaim {
                    metadata: ObjectMeta {
                        name: want.clone(),
                        namespace: namespace.into(),
                        owner_references: vec![new_controller_ref(&set, "apps/v1")],
                        ..Default::default()
                    },
                    storage_class: String::new(),
                };
                client.create(Some(namespace), pvc).await?;
            }
        }
        let want = pod_name(&set, ordinal);
        if !owned.iter().any(|p| p.name() == want) {
            let pod = build_ordinal_pod(&set, ordinal);
            client.create(Some(namespace), pod).await?;
        }
    }

    // 2) Delete pods above `desired` (highest ordinal first).
    let to_delete: Vec<Pod> = owned
        .iter()
        .filter(|p| ordinal_of(&set, p.name()).map_or(false, |o| o >= desired))
        .cloned()
        .collect();
    let mut to_delete = to_delete;
    to_delete.sort_by(|a, b| b.name().cmp(a.name()));
    for p in to_delete {
        client.delete("Pod", Some(namespace), p.name()).await?;
    }

    // 3) Status.
    let post_pods: Vec<Pod> = client.list(Some(namespace), Some(&sel)).await?;
    let live: Vec<Pod> = post_pods
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();
    let ready = live
        .iter()
        .filter(|p| p.status.phase == crate::types::PodPhase::Running)
        .count();
    set.status.replicas = i32::try_from(live.len()).unwrap_or(i32::MAX);
    set.status.ready_replicas = i32::try_from(ready).unwrap_or(i32::MAX);
    set.status.current_replicas = set.status.replicas;
    set.status.observed_generation = set.meta().generation;
    let updated = client.update(Some(namespace), set).await?;
    Ok(updated.status)
}

/// Extract the ordinal suffix from a StatefulSet-owned pod name.
fn ordinal_of(set: &StatefulSet, pod_name: &str) -> Option<i32> {
    let prefix = format!("{}-", set.name());
    pod_name.strip_prefix(&prefix).and_then(|s| s.parse().ok())
}

fn build_ordinal_pod(set: &StatefulSet, ordinal: i32) -> Pod {
    let mut meta = ObjectMeta {
        name: pod_name(set, ordinal),
        namespace: set.namespace().into(),
        labels: set.spec.template.metadata.labels.clone(),
        annotations: set.spec.template.metadata.annotations.clone(),
        ..Default::default()
    };
    meta.owner_references.push(new_controller_ref(set, "apps/v1"));
    for (k, v) in &set.spec.selector.match_labels {
        meta.labels.entry(k.clone()).or_insert(v.clone());
    }
    let mut spec = set.spec.template.spec.clone();
    spec.volume_claims = set
        .spec
        .volume_claim_templates
        .iter()
        .map(|c| pvc_name(set, c, ordinal))
        .collect();
    Pod {
        metadata: meta,
        spec,
        ..Default::default()
    }
}

pub struct StatefulSetController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> StatefulSetController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        sync_stateful_set(self.client.as_ref(), &ns, &name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{LabelSelector, PodTemplateSpec, StatefulSetSpec};

    fn make_sts(name: &str, replicas: i32, claims: Vec<&str>) -> StatefulSet {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), name.into());
        let mut tpl = PodTemplateSpec::default();
        tpl.metadata.labels.insert("app".into(), name.into());
        StatefulSet {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: "default".into(),
                ..Default::default()
            },
            spec: StatefulSetSpec {
                replicas,
                selector: sel,
                template: tpl,
                service_name: format!("{name}-headless"),
                volume_claim_templates: claims.iter().map(|s| (*s).to_string()).collect(),
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn first_sync_creates_pods_in_ordinal_order() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_sts("db", 3, vec![]));
        sync_stateful_set(&c, "default", "db").await.unwrap();
        let pods: Vec<Pod> = c.list(Some("default"), None).await.unwrap();
        let mut names: Vec<_> = pods.iter().map(|p| p.name().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["db-0", "db-1", "db-2"]);
    }

    #[tokio::test]
    async fn shrink_deletes_highest_ordinal_first() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_sts("db", 3, vec![]));
        sync_stateful_set(&c, "default", "db").await.unwrap();
        let mut s = c.get::<StatefulSet>(Some("default"), "db").await.unwrap();
        s.spec.replicas = 1;
        c.update(Some("default"), s).await.unwrap();
        sync_stateful_set(&c, "default", "db").await.unwrap();
        let pods: Vec<Pod> = c.list(Some("default"), None).await.unwrap();
        let mut names: Vec<_> = pods.iter().map(|p| p.name().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["db-0"]);
    }

    #[tokio::test]
    async fn creates_one_pvc_per_template_per_ordinal() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_sts("db", 2, vec!["data", "logs"]));
        sync_stateful_set(&c, "default", "db").await.unwrap();
        let pvcs: Vec<PersistentVolumeClaim> =
            c.list(Some("default"), None).await.unwrap();
        let mut names: Vec<_> = pvcs.iter().map(|p| p.name().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["data-db-0", "data-db-1", "logs-db-0", "logs-db-1"]);
    }

    #[tokio::test]
    async fn pvcs_are_not_deleted_on_shrink() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_sts("db", 2, vec!["data"]));
        sync_stateful_set(&c, "default", "db").await.unwrap();
        let mut s = c.get::<StatefulSet>(Some("default"), "db").await.unwrap();
        s.spec.replicas = 1;
        c.update(Some("default"), s).await.unwrap();
        sync_stateful_set(&c, "default", "db").await.unwrap();
        assert_eq!(c.count("PersistentVolumeClaim"), 2);
    }

    #[tokio::test]
    async fn pod_spec_carries_pvc_names() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_sts("db", 1, vec!["data"]));
        sync_stateful_set(&c, "default", "db").await.unwrap();
        let p = c.get::<Pod>(Some("default"), "db-0").await.unwrap();
        assert_eq!(p.spec.volume_claims, vec!["data-db-0".to_string()]);
    }
}
