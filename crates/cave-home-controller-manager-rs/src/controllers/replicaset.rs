// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/replicaset/replica_set.go
//         pkg/controller/replicaset/replica_set_utils.go
//
//! ReplicaSetController.
//!
//! Watches `ReplicaSet` + `Pod` and reconciles every RS such that
//! `len(pods_matching_selector_and_owned) == spec.replicas`.

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient, LabelSelectorFilter};
use crate::types::{
    is_controlled_by, new_controller_ref, KubeResource, ObjectMeta, Pod, ReplicaSet,
};

/// One reconcile sweep over the ReplicaSet identified by `(namespace, name)`.
///
/// Mirrors `ReplicaSetController.syncReplicaSet`. Returns the post-sync
/// status (`ReplicaSetStatus`) on success.
pub async fn sync_replica_set<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    name: &str,
) -> ApiResult<crate::types::ReplicaSetStatus> {
    let mut rs: ReplicaSet = client.get(Some(namespace), name).await?;
    let owner_uid = rs.uid().clone();

    // 1) List pods in the namespace and filter to those owned by this RS.
    //    Upstream uses `manageReplicas` after an adoption / orphan-release
    //    pass; Phase 2 only ports the "owned + selector match" half because
    //    the InMemoryApiClient never produces orphans.
    let sel = LabelSelectorFilter::from(&rs.spec.selector);
    let candidate_pods: Vec<Pod> = client.list(Some(namespace), Some(&sel)).await?;
    let mut owned: Vec<Pod> = candidate_pods
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();

    // 2) Compute diff. `manageReplicas` upstream caps each sync at
    //    `BurstReplicas` (default 500); Phase 2 keeps the same shape but the
    //    InMemoryApiClient never benefits from batching.
    let current = i32::try_from(owned.len()).unwrap_or(i32::MAX);
    let desired = rs.spec.replicas.max(0);
    if current < desired {
        let diff = desired - current;
        for i in 0..diff {
            let pod = build_pod_from_template(&rs, current + i);
            client.create(Some(namespace), pod).await?;
        }
    } else if current > desired {
        let diff = (current - desired) as usize;
        // Upstream sorts by `getPodsToDelete`: not-ready > ready,
        // youngest > oldest, etc. Phase 2 deletes from the tail (deterministic
        // for tests against InMemoryApiClient).
        owned.sort_by(|a, b| a.name().cmp(b.name()));
        for p in owned.iter().rev().take(diff) {
            client.delete("Pod", Some(namespace), p.name()).await?;
        }
    }

    // 3) Update status — `calculateStatus`.
    let post_sel = LabelSelectorFilter::from(&rs.spec.selector);
    let post_pods: Vec<Pod> = client.list(Some(namespace), Some(&post_sel)).await?;
    let live_owned: Vec<Pod> = post_pods
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();
    let ready = live_owned
        .iter()
        .filter(|p| p.status.phase == crate::types::PodPhase::Running)
        .count();
    rs.status.replicas = i32::try_from(live_owned.len()).unwrap_or(i32::MAX);
    rs.status.ready_replicas = i32::try_from(ready).unwrap_or(i32::MAX);
    rs.status.available_replicas = rs.status.ready_replicas;
    rs.status.observed_generation = rs.meta().generation;
    let updated = client.update(Some(namespace), rs).await?;
    Ok(updated.status)
}

/// Mirrors `controller.GetPodFromTemplate` — fabricate a `Pod` that inherits
/// the RS's template + controller owner-ref.
pub(crate) fn build_pod_from_template(rs: &ReplicaSet, ordinal: i32) -> Pod {
    // Upstream uses a generateName + uid suffix; Phase 2 uses
    // `<rs-name>-<ordinal>` so the test assertions can predict the name.
    let mut meta = ObjectMeta {
        name: format!("{}-{ordinal}", rs.name()),
        namespace: rs.namespace().into(),
        labels: rs.spec.template.metadata.labels.clone(),
        annotations: rs.spec.template.metadata.annotations.clone(),
        ..Default::default()
    };
    meta.owner_references
        .push(new_controller_ref(rs, "apps/v1"));
    // Merge selector labels into pod labels so the next list() finds it
    // back — upstream does this via `LabelSelectorAsSelector` over the
    // template metadata, which RS validation enforces upfront.
    for (k, v) in &rs.spec.selector.match_labels {
        meta.labels.entry(k.clone()).or_insert(v.clone());
    }
    Pod {
        metadata: meta,
        spec: rs.spec.template.spec.clone(),
        ..Default::default()
    }
}

/// Convenience handle held by [`crate::manager::ControllerManager`].
pub struct ReplicaSetController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> ReplicaSetController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    /// Reconcile the RS identified by `key = "namespace/name"`.
    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        sync_replica_set(self.client.as_ref(), &ns, &name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{LabelSelector, PodTemplateSpec};

    fn make_rs(name: &str, replicas: i32) -> ReplicaSet {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), name.into());
        let mut tpl = PodTemplateSpec::default();
        tpl.metadata.labels.insert("app".into(), name.into());
        ReplicaSet {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: "default".into(),
                ..Default::default()
            },
            spec: crate::types::ReplicaSetSpec {
                replicas,
                selector: sel,
                template: tpl,
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn scales_up_to_match_replicas() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_rs("web", 3));
        sync_replica_set(&c, "default", "web").await.unwrap();
        assert_eq!(c.count("Pod"), 3);
    }

    #[tokio::test]
    async fn scales_down_excess_pods() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_rs("web", 1));
        sync_replica_set(&c, "default", "web").await.unwrap();
        // Now reduce desired to 0 via a second reconcile.
        let mut rs = c.get::<ReplicaSet>(Some("default"), "web").await.unwrap();
        rs.spec.replicas = 0;
        c.update(Some("default"), rs).await.unwrap();
        sync_replica_set(&c, "default", "web").await.unwrap();
        assert_eq!(c.count("Pod"), 0);
    }

    #[tokio::test]
    async fn idempotent_when_already_in_desired_state() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_rs("web", 2));
        sync_replica_set(&c, "default", "web").await.unwrap();
        sync_replica_set(&c, "default", "web").await.unwrap();
        assert_eq!(c.count("Pod"), 2);
    }

    #[tokio::test]
    async fn status_reports_observed_generation() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_rs("web", 1));
        let status = sync_replica_set(&c, "default", "web").await.unwrap();
        assert_eq!(status.replicas, 1);
        assert!(status.observed_generation >= 1);
    }
}
