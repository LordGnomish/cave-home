// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/job/job_controller.go
//         pkg/controller/job/utils.go
//
//! JobController.
//!
//! Reconciles a Job towards `spec.completions` successful pod terminations,
//! creating up to `spec.parallelism` running pods at a time. The Phase 2
//! port honours the `Indexed` completion mode informally — pods are named
//! `<job>-<index>` and `index` is recorded as an annotation.

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient, LabelSelectorFilter};
use crate::types::{
    is_controlled_by, new_controller_ref, Job, KubeResource, ObjectMeta, Pod, PodPhase,
};

/// Annotation that records the job-completion index on each owned pod.
pub const JOB_COMPLETION_INDEX_ANNOTATION: &str = "batch.kubernetes.io/job-completion-index";

/// Mirrors `Controller.syncJob`.
pub async fn sync_job<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    name: &str,
) -> ApiResult<crate::types::JobStatus> {
    let mut job: Job = client.get(Some(namespace), name).await?;
    let owner_uid = job.uid().clone();

    let sel = LabelSelectorFilter::from(&job.spec.selector);
    let pods: Vec<Pod> = client.list(Some(namespace), Some(&sel)).await?;
    let owned: Vec<Pod> = pods
        .into_iter()
        .filter(|p| is_controlled_by(p.meta(), &owner_uid))
        .collect();

    let succeeded = owned
        .iter()
        .filter(|p| p.status.phase == PodPhase::Succeeded)
        .count() as i32;
    let failed = owned
        .iter()
        .filter(|p| p.status.phase == PodPhase::Failed)
        .count() as i32;
    let active_pods: Vec<&Pod> = owned
        .iter()
        .filter(|p| matches!(p.status.phase, PodPhase::Pending | PodPhase::Running))
        .collect();
    let active = active_pods.len() as i32;

    let completions = job.spec.completions.max(0);
    let parallelism = job.spec.parallelism.max(0);
    let backoff_limit = job.spec.backoff_limit.max(0);

    let mut new_active = active;
    let new_succeeded = succeeded;
    let new_failed = failed;

    // Two terminal conditions: completed (succeeded >= completions) and
    // backoff exhausted.
    let backoff_exceeded = backoff_limit > 0 && failed >= backoff_limit;
    let completed = completions > 0 && succeeded >= completions;
    if backoff_exceeded {
        // Reap any remaining active pods so the job doesn't linger.
        for p in &active_pods {
            client.delete("Pod", Some(namespace), p.name()).await?;
        }
        new_active = 0;
    } else if !completed {
        // Compute how many more pods to create this sync.
        let needed_more_completions = (completions - succeeded).max(0);
        let target_parallel = if completions == 0 {
            parallelism
        } else {
            std::cmp::min(parallelism, needed_more_completions)
        };
        let diff = (target_parallel - active).max(0);
        // Allocate the next set of completion indices that aren't already in
        // flight or succeeded.
        let mut taken = std::collections::BTreeSet::new();
        for p in &owned {
            if let Some(ix) = p
                .meta()
                .annotations
                .get(JOB_COMPLETION_INDEX_ANNOTATION)
                .and_then(|s| s.parse::<i32>().ok())
            {
                taken.insert(ix);
            }
        }
        let mut created = 0;
        let mut ix = 0;
        while created < diff {
            if !taken.contains(&ix) {
                let pod = build_pod(&job, ix);
                client.create(Some(namespace), pod).await?;
                taken.insert(ix);
                created += 1;
                new_active += 1;
            }
            ix += 1;
            if ix > completions.max(parallelism) + 1024 {
                break; // safety bound — should never trip in tests
            }
        }
    }

    job.status.active = new_active;
    job.status.succeeded = new_succeeded;
    job.status.failed = new_failed;
    job.status.completed = completed;

    let _ = (new_succeeded, new_failed); // keep tracker accumulators legible
    client.update(Some(namespace), job).await
        .map(|j| j.status)
}

fn build_pod(job: &Job, index: i32) -> Pod {
    let mut meta = ObjectMeta {
        name: format!("{}-{index}", job.name()),
        namespace: job.namespace().into(),
        labels: job.spec.template.metadata.labels.clone(),
        annotations: job.spec.template.metadata.annotations.clone(),
        ..Default::default()
    };
    meta.annotations.insert(
        JOB_COMPLETION_INDEX_ANNOTATION.into(),
        index.to_string(),
    );
    meta.owner_references.push(new_controller_ref(job, "batch/v1"));
    for (k, v) in &job.spec.selector.match_labels {
        meta.labels.entry(k.clone()).or_insert(v.clone());
    }
    let mut spec = job.spec.template.spec.clone();
    spec.restart_policy = crate::types::RestartPolicy::Never;
    Pod {
        metadata: meta,
        spec,
        ..Default::default()
    }
}

pub struct JobController<C: ControllerApiClient> {
    client: Arc<C>,
}

impl<C: ControllerApiClient> JobController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }

    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        sync_job(self.client.as_ref(), &ns, &name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{JobSpec, LabelSelector, PodTemplateSpec};

    fn make_job(name: &str, parallelism: i32, completions: i32) -> Job {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("job".into(), name.into());
        let mut tpl = PodTemplateSpec::default();
        tpl.metadata.labels.insert("job".into(), name.into());
        Job {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: "default".into(),
                ..Default::default()
            },
            spec: JobSpec {
                parallelism,
                completions,
                backoff_limit: 6,
                selector: sel,
                template: tpl,
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn first_sync_creates_up_to_parallelism_pods() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_job("compute", 3, 6));
        sync_job(&c, "default", "compute").await.unwrap();
        assert_eq!(c.count("Pod"), 3);
    }

    #[tokio::test]
    async fn sync_does_not_exceed_remaining_completions() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_job("compute", 10, 2));
        sync_job(&c, "default", "compute").await.unwrap();
        assert_eq!(c.count("Pod"), 2);
    }

    #[tokio::test]
    async fn second_sync_replenishes_after_success() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_job("compute", 1, 3));
        sync_job(&c, "default", "compute").await.unwrap();
        assert_eq!(c.count("Pod"), 1);
        // Mark the running pod as succeeded.
        let mut p = c.get::<Pod>(Some("default"), "compute-0").await.unwrap();
        p.status.phase = PodPhase::Succeeded;
        c.update(Some("default"), p).await.unwrap();
        sync_job(&c, "default", "compute").await.unwrap();
        // Now we have 1 succeeded pod + 1 new active pod (index 1).
        assert_eq!(c.count("Pod"), 2);
    }

    #[tokio::test]
    async fn sync_marks_completed_when_all_succeed() {
        let c = InMemoryApiClient::new();
        c.seed(Some("default"), make_job("compute", 1, 1));
        sync_job(&c, "default", "compute").await.unwrap();
        let mut p = c.get::<Pod>(Some("default"), "compute-0").await.unwrap();
        p.status.phase = PodPhase::Succeeded;
        c.update(Some("default"), p).await.unwrap();
        let status = sync_job(&c, "default", "compute").await.unwrap();
        assert!(status.completed);
        assert_eq!(status.succeeded, 1);
    }

    #[tokio::test]
    async fn backoff_exceeded_deletes_active_pods() {
        let c = InMemoryApiClient::new();
        let mut j = make_job("flaky", 2, 10);
        j.spec.backoff_limit = 2;
        c.seed(Some("default"), j);
        sync_job(&c, "default", "flaky").await.unwrap();
        // Mark all current pods as failed; that crosses backoff_limit=2.
        for ix in 0..2i32 {
            let mut p = c
                .get::<Pod>(Some("default"), &format!("flaky-{ix}"))
                .await
                .unwrap();
            p.status.phase = PodPhase::Failed;
            c.update(Some("default"), p).await.unwrap();
        }
        sync_job(&c, "default", "flaky").await.unwrap();
        let pods: Vec<Pod> = c.list(Some("default"), None).await.unwrap();
        let active = pods
            .iter()
            .filter(|p| matches!(p.status.phase, PodPhase::Pending | PodPhase::Running))
            .count();
        assert_eq!(active, 0);
    }
}
