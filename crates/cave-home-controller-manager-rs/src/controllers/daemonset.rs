// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/daemon/daemon_controller.go
//         pkg/controller/daemon/update.go
//
//! DaemonSetController.
//!
//! For every node that matches the DaemonSet's node-affinity (Phase 2: all
//! schedulable, non-tainted nodes), ensure exactly one pod owned by the DS
//! exists on that node.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient, LabelSelectorFilter};
use crate::types::{
    is_controlled_by, new_controller_ref, DaemonSet, KubeResource, Node, ObjectMeta, Pod, TaintEffect,
};

/// One reconcile pass.
///
/// Mirrors `DaemonSetsController.syncDaemonSet`. Phase 2 does NOT honour
/// `Affinity`/`NodeSelector` fields beyond "node has no `NoSchedule` taint";
/// affinity is recorded in `parity.manifest.toml` as `[[unmapped]]`.
pub async fn sync_daemon_set<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    name: &str,
) -> ApiResult<crate::types::DaemonSetStatus> {
    let mut ds: DaemonSet = client.get(Some(namespace), name).await?;
    let owner_uid = ds.uid().clone();

    // 1) Eligible nodes — skip nodes that have NoSchedule taints or are
    //    marked unschedulable. (Real DS controller respects tolerations;
    //    Phase 2 records that as `[[unmapped]]`.)
    let nodes: Vec<Node> = client.list(None, None).await?;
    let eligible: Vec<&Node> = nodes
        .iter()
        .filter(|n| !n.spec.unschedulable)
        .filter(|n| {
            !n.spec
                .taints
                .iter()
                .any(|t| matches!(t.effect, TaintEffect::NoSchedule | TaintEffect::NoExecute))
        })
        .collect();
    let eligible_names: BTreeSet<String> =
        eligible.iter().map(|n| n.name().to_string()).collect();

    // 2) Pods currently owned by this DS, keyed by `node_name`.
    let sel = LabelSelectorFilter::from(&ds.spec.selector);
    let pods: Vec<Pod> = client.list(Some(namespace), Some(&sel)).await?;
    let owned: Vec<Pod> = pods
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();
    let owned_node_names: BTreeSet<String> = owned
        .iter()
        .map(|p| p.spec.node_name.clone())
        .filter(|n| !n.is_empty())
        .collect();

    // 3) Create on every eligible node that lacks a pod.
    for node in &eligible {
        if owned_node_names.contains(node.name()) {
            continue;
        }
        let pod = build_pod_for_node(&ds, node.name());
        client.create(Some(namespace), pod).await?;
    }

    // 4) Delete pods on nodes that are no longer eligible.
    for pod in &owned {
        if !pod.spec.node_name.is_empty() && !eligible_names.contains(&pod.spec.node_name) {
            client.delete("Pod", Some(namespace), pod.name()).await?;
        }
    }

    // 5) Status — `manage()` post-step.
    let post: Vec<Pod> = client.list(Some(namespace), Some(&sel)).await?;
    let live: Vec<Pod> = post
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();
    let scheduled = live
        .iter()
        .filter(|p| eligible_names.contains(&p.spec.node_name))
        .count();
    let ready = live
        .iter()
        .filter(|p| p.status.phase == crate::types::PodPhase::Running)
        .count();
    ds.status.desired_number_scheduled = i32::try_from(eligible_names.len()).unwrap_or(i32::MAX);
    ds.status.current_number_scheduled = i32::try_from(scheduled).unwrap_or(i32::MAX);
    ds.status.number_ready = i32::try_from(ready).unwrap_or(i32::MAX);
    ds.status.observed_generation = ds.meta().generation;
    let updated = client.update(Some(namespace), ds).await?;
    Ok(updated.status)
}

fn build_pod_for_node(ds: &DaemonSet, node_name: &str) -> Pod {
    let mut meta = ObjectMeta {
        name: format!("{}-{node_name}", ds.name()),
        namespace: ds.namespace().into(),
        labels: ds.spec.template.metadata.labels.clone(),
        annotations: ds.spec.template.metadata.annotations.clone(),
        ..Default::default()
    };
    meta.owner_references.push(new_controller_ref(ds, "apps/v1"));
    for (k, v) in &ds.spec.selector.match_labels {
        meta.labels.entry(k.clone()).or_insert(v.clone());
    }
    let mut spec = ds.spec.template.spec.clone();
    spec.node_name = node_name.to_string();
    Pod {
        metadata: meta,
        spec,
        ..Default::default()
    }
}

pub struct DaemonSetController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> DaemonSetController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        sync_daemon_set(self.client.as_ref(), &ns, &name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{
        DaemonSetSpec, LabelSelector, NodeSpec, PodTemplateSpec, Taint, TaintEffect,
    };

    fn make_ds(name: &str) -> DaemonSet {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), name.into());
        let mut tpl = PodTemplateSpec::default();
        tpl.metadata.labels.insert("app".into(), name.into());
        DaemonSet {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: "default".into(),
                ..Default::default()
            },
            spec: DaemonSetSpec {
                selector: sel,
                template: tpl,
            },
            ..Default::default()
        }
    }

    fn make_node(name: &str) -> Node {
        Node {
            metadata: ObjectMeta {
                name: name.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn creates_one_pod_per_eligible_node() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1"));
        c.seed(None, make_node("n2"));
        c.seed(None, make_node("n3"));
        c.seed(Some("default"), make_ds("logger"));
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        assert_eq!(c.count("Pod"), 3);
    }

    #[tokio::test]
    async fn skips_unschedulable_nodes() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1"));
        let mut n2 = make_node("n2");
        n2.spec = NodeSpec {
            unschedulable: true,
            ..Default::default()
        };
        c.seed(None, n2);
        c.seed(Some("default"), make_ds("logger"));
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        assert_eq!(c.count("Pod"), 1);
    }

    #[tokio::test]
    async fn skips_no_schedule_tainted_nodes() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1"));
        let mut n2 = make_node("n2");
        n2.spec.taints.push(Taint {
            key: "node.kubernetes.io/unreachable".into(),
            value: String::new(),
            effect: TaintEffect::NoSchedule,
        });
        c.seed(None, n2);
        c.seed(Some("default"), make_ds("logger"));
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        assert_eq!(c.count("Pod"), 1);
    }

    #[tokio::test]
    async fn deletes_pod_when_node_disappears() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1"));
        c.seed(None, make_node("n2"));
        c.seed(Some("default"), make_ds("logger"));
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        assert_eq!(c.count("Pod"), 2);
        let mut n2 = c.get::<Node>(None, "n2").await.unwrap();
        n2.spec.unschedulable = true;
        c.update(None, n2).await.unwrap();
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        assert_eq!(c.count("Pod"), 1);
    }

    #[tokio::test]
    async fn idempotent_when_already_in_desired_state() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1"));
        c.seed(Some("default"), make_ds("logger"));
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        sync_daemon_set(&c, "default", "logger").await.unwrap();
        assert_eq!(c.count("Pod"), 1);
    }
}
