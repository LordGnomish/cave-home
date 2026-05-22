// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/deployment/deployment_controller.go
//         pkg/controller/deployment/sync.go
//         pkg/controller/deployment/rolling.go
//         pkg/controller/deployment/util/deployment_util.go
//
//! DeploymentController.
//!
//! Owns `ReplicaSet`s. On every spec change the controller stamps a new
//! "pod-template-hash" label and either creates a new RS (rolling update) or
//! resizes the existing one. Old RSs are scaled to zero but kept around for
//! `revisionHistoryLimit`.

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::types::{
    is_controlled_by, new_controller_ref, Deployment, KubeResource, LabelSelector, ObjectMeta,
    ReplicaSet, ReplicaSetSpec,
};

/// Annotation that records the deployment's monotonic revision number on each
/// of its RSs. Mirrors `DefaultDeploymentUniqueLabelKey` /
/// `RevisionAnnotation` upstream.
pub const REVISION_ANNOTATION: &str = "deployment.kubernetes.io/revision";

/// Label that distinguishes RSs belonging to the same Deployment but with
/// different pod templates. Mirrors `DefaultDeploymentUniqueLabelKey`.
pub const POD_TEMPLATE_HASH_LABEL: &str = "pod-template-hash";

/// Deterministic hash of the deployment's pod template.
///
/// Upstream uses a hashing scheme over `PodTemplateSpec` (FNV-32 of the
/// gob-serialised template). The Phase 2 port reproduces only the relevant
/// observable property — same template => same hash, different template =>
/// different hash — by hashing the names + images + label map.
fn pod_template_hash(d: &Deployment) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for c in &d.spec.template.spec.containers {
        c.name.hash(&mut h);
        c.image.hash(&mut h);
        c.command.hash(&mut h);
        c.args.hash(&mut h);
    }
    for (k, v) in &d.spec.template.metadata.labels {
        k.hash(&mut h);
        v.hash(&mut h);
    }
    format!("{:x}", h.finish())
}

/// `sync_deployment` — one reconcile.
///
/// Mirrors `DeploymentController.syncDeployment` for the RollingUpdate path
/// (Recreate is `[[unmapped]]`).
pub async fn sync_deployment<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    name: &str,
) -> ApiResult<()> {
    let mut deploy: Deployment = client.get(Some(namespace), name).await?;

    // 1) List all RSs owned by this deployment.
    let owner_uid = deploy.uid().clone();
    let all_rs: Vec<ReplicaSet> = client.list(Some(namespace), None).await?;
    let owned: Vec<ReplicaSet> = all_rs
        .into_iter()
        .filter(|rs| is_controlled_by(rs.meta(), &owner_uid))
        .collect();

    // 2) Compute pod-template-hash for current spec.
    let target_hash = pod_template_hash(&deploy);

    // 3) Find or create the "new" RS — the one matching `target_hash`.
    let new_rs_existing = owned.iter().find(|rs| {
        rs.meta()
            .labels
            .get(POD_TEMPLATE_HASH_LABEL)
            .map_or(false, |h| h == &target_hash)
    });

    let (new_rs_name, new_revision) = if let Some(rs) = new_rs_existing {
        let rev: u64 = rs
            .meta()
            .annotations
            .get(REVISION_ANNOTATION)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        (rs.name().to_string(), rev)
    } else {
        // Allocate next revision.
        let next_rev: u64 = owned
            .iter()
            .filter_map(|rs| rs.meta().annotations.get(REVISION_ANNOTATION))
            .filter_map(|s| s.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let rs = new_replica_set_from_deployment(&deploy, &target_hash, next_rev);
        let created = client.create(Some(namespace), rs).await?;
        (created.name().to_string(), next_rev)
    };

    // 4) Scale: surge until new RS reaches desired, then scale old RSs down
    //    one step per sync (the upstream "rolling" path). Phase 2 uses
    //    max_surge/max_unavailable as absolute integers; percentage form is
    //    `[[unmapped]]`.
    let desired = deploy.spec.replicas.max(0);
    let max_surge = deploy.spec.strategy.rolling_update.max_surge.max(0);
    let max_unavailable = deploy
        .spec
        .strategy
        .rolling_update
        .max_unavailable
        .max(0);

    // Refresh RSs (the create above mutated the world).
    let all_rs: Vec<ReplicaSet> = client.list(Some(namespace), None).await?;
    let owned: Vec<ReplicaSet> = all_rs
        .into_iter()
        .filter(|rs| is_controlled_by(rs.meta(), &owner_uid))
        .collect();

    let new_rs = owned
        .iter()
        .find(|rs| rs.name() == new_rs_name)
        .cloned()
        .expect("just-created RS must exist");
    let old_rs_total: i32 = owned
        .iter()
        .filter(|rs| rs.name() != new_rs_name)
        .map(|rs| rs.spec.replicas)
        .sum();

    let max_total = desired.saturating_add(max_surge);
    let new_target = std::cmp::min(desired, max_total.saturating_sub(old_rs_total));
    let new_target = new_target.max(0);
    if new_rs.spec.replicas != new_target {
        let mut updated = new_rs.clone();
        updated.spec.replicas = new_target;
        client.update(Some(namespace), updated).await?;
    }

    // Scale old RSs down to enforce maxUnavailable.
    // `available_old + new_target >= desired - maxUnavailable`.
    let allowed_unavailable = max_unavailable;
    let mut old_total_remaining = old_rs_total;
    for rs in owned.iter().filter(|rs| rs.name() != new_rs_name) {
        if rs.spec.replicas == 0 {
            continue;
        }
        let min_old = (desired - new_target - allowed_unavailable).max(0);
        let scale_to = std::cmp::min(rs.spec.replicas, (min_old - (old_total_remaining - rs.spec.replicas)).max(0));
        let scale_to = scale_to.max(0);
        if rs.spec.replicas != scale_to {
            let mut updated = rs.clone();
            updated.spec.replicas = scale_to;
            client.update(Some(namespace), updated).await?;
            old_total_remaining -= rs.spec.replicas - scale_to;
        }
    }

    // 5) Roll the deployment-level status.
    let _ = new_revision;
    let post: Vec<ReplicaSet> = client.list(Some(namespace), None).await?;
    let owned: Vec<ReplicaSet> = post
        .into_iter()
        .filter(|rs| is_controlled_by(rs.meta(), &owner_uid))
        .collect();
    let total_replicas: i32 = owned.iter().map(|rs| rs.status.replicas).sum();
    let updated_replicas = owned
        .iter()
        .find(|rs| rs.name() == new_rs_name)
        .map_or(0, |rs| rs.status.replicas);
    let ready_replicas: i32 = owned.iter().map(|rs| rs.status.ready_replicas).sum();
    let available_replicas: i32 = owned.iter().map(|rs| rs.status.available_replicas).sum();

    deploy.status.observed_generation = deploy.meta().generation;
    deploy.status.replicas = total_replicas;
    deploy.status.updated_replicas = updated_replicas;
    deploy.status.ready_replicas = ready_replicas;
    deploy.status.available_replicas = available_replicas;
    deploy.status.unavailable_replicas = (desired - available_replicas).max(0);
    client.update(Some(namespace), deploy).await?;

    // 6) GC excess revisions per `revisionHistoryLimit`.
    cleanup_revisions(client, namespace, &owner_uid, &new_rs_name, desired, deploy_history_limit(&owned)).await?;

    Ok(())
}

fn deploy_history_limit(_owned: &[ReplicaSet]) -> i32 {
    // Upstream default is 10.
    10
}

async fn cleanup_revisions<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    owner_uid: &crate::types::Uid,
    new_rs_name: &str,
    desired: i32,
    history_limit: i32,
) -> ApiResult<()> {
    let _ = desired;
    let all_rs: Vec<ReplicaSet> = client.list(Some(namespace), None).await?;
    let mut owned: Vec<ReplicaSet> = all_rs
        .into_iter()
        .filter(|rs| is_controlled_by(rs.meta(), owner_uid))
        .filter(|rs| rs.name() != new_rs_name)
        .filter(|rs| rs.spec.replicas == 0)
        .collect();
    owned.sort_by_key(|rs| {
        rs.meta()
            .annotations
            .get(REVISION_ANNOTATION)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    let excess = owned.len() as i32 - history_limit;
    if excess <= 0 {
        return Ok(());
    }
    for rs in owned.iter().take(excess as usize) {
        client
            .delete("ReplicaSet", Some(namespace), rs.name())
            .await?;
    }
    Ok(())
}

/// `getNewReplicaSet`-style RS fabrication.
fn new_replica_set_from_deployment(
    d: &Deployment,
    template_hash: &str,
    revision: u64,
) -> ReplicaSet {
    let mut labels = d.spec.template.metadata.labels.clone();
    labels.insert(POD_TEMPLATE_HASH_LABEL.into(), template_hash.into());

    let mut selector = LabelSelector::default();
    for (k, v) in &d.spec.selector.match_labels {
        selector.match_labels.insert(k.clone(), v.clone());
    }
    selector
        .match_labels
        .insert(POD_TEMPLATE_HASH_LABEL.into(), template_hash.into());

    let mut tpl = d.spec.template.clone();
    tpl.metadata.labels = labels.clone();

    let mut meta = ObjectMeta {
        name: format!("{}-{template_hash}", d.name()),
        namespace: d.namespace().into(),
        labels: labels.clone(),
        ..Default::default()
    };
    meta.annotations
        .insert(REVISION_ANNOTATION.into(), revision.to_string());
    meta.owner_references
        .push(new_controller_ref(d, "apps/v1"));

    ReplicaSet {
        metadata: meta,
        spec: ReplicaSetSpec {
            replicas: 0, // ramp-up handled by the sync loop
            selector,
            template: tpl,
        },
        status: crate::types::ReplicaSetStatus::default(),
    }
}

/// Phase 2 handle.
pub struct DeploymentController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> DeploymentController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        sync_deployment(self.client.as_ref(), &ns, &name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{Container, DeploymentSpec, PodTemplateSpec};

    fn make_deploy(name: &str, replicas: i32, image: &str) -> Deployment {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), name.into());
        let mut tpl = PodTemplateSpec::default();
        tpl.metadata.labels.insert("app".into(), name.into());
        tpl.spec.containers.push(Container {
            name: name.into(),
            image: image.into(),
            ..Default::default()
        });
        Deployment {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: "default".into(),
                ..Default::default()
            },
            spec: DeploymentSpec {
                replicas,
                selector: sel,
                template: tpl,
                strategy: crate::types::DeploymentStrategy {
                    kind: crate::types::DeploymentStrategyType::RollingUpdate,
                    rolling_update: crate::types::RollingUpdateDeployment {
                        max_unavailable: 1,
                        max_surge: 1,
                    },
                },
                revision_history_limit: 10,
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn first_sync_creates_replicaset_at_full_replicas() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_deploy("web", 3, "nginx:1"));
        sync_deployment(&c, "default", "web").await.unwrap();
        let rss: Vec<ReplicaSet> = c.list(Some("default"), None).await.unwrap();
        assert_eq!(rss.len(), 1);
        assert_eq!(rss[0].spec.replicas, 3);
    }

    #[tokio::test]
    async fn second_sync_with_unchanged_template_does_not_create_new_rs() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_deploy("web", 3, "nginx:1"));
        sync_deployment(&c, "default", "web").await.unwrap();
        sync_deployment(&c, "default", "web").await.unwrap();
        let rss: Vec<ReplicaSet> = c.list(Some("default"), None).await.unwrap();
        assert_eq!(rss.len(), 1);
    }

    #[tokio::test]
    async fn template_change_creates_new_rs_and_scales_old_down() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_deploy("web", 3, "nginx:1"));
        sync_deployment(&c, "default", "web").await.unwrap();
        let mut d = c.get::<Deployment>(Some("default"), "web").await.unwrap();
        d.spec.template.spec.containers[0].image = "nginx:2".into();
        c.update(Some("default"), d).await.unwrap();
        sync_deployment(&c, "default", "web").await.unwrap();
        let rss: Vec<ReplicaSet> = c.list(Some("default"), None).await.unwrap();
        assert_eq!(rss.len(), 2);
        let total: i32 = rss.iter().map(|r| r.spec.replicas).sum();
        // surge=1 maxUnavailable=1 desired=3 -> total in flight should be in
        // [desired-maxUnavailable, desired+maxSurge].
        assert!(total >= 3 - 1 && total <= 3 + 1, "total {total}");
    }

    #[tokio::test]
    async fn deployment_status_observed_generation_matches() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_deploy("web", 1, "nginx:1"));
        sync_deployment(&c, "default", "web").await.unwrap();
        let d = c.get::<Deployment>(Some("default"), "web").await.unwrap();
        assert!(d.status.observed_generation >= 1);
    }
}
