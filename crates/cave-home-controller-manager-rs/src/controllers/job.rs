// SPDX-License-Identifier: Apache-2.0
//! Job controller — runs pods until a Job reaches its completion count.
//!
//! Behavioural reimplementation of the documented `pkg/controller/job`
//! contract, reconciling against the in-memory apiserver:
//!
//! * count owned pods by phase (succeeded / failed / active);
//! * **complete** the Job once `succeeded >= completions` (record
//!   `finished_at`, delete any leftover active pods);
//! * **fail** the Job once `failed > backoff_limit` (record `finished_at`,
//!   delete active pods);
//! * otherwise create pods up to `min(parallelism, completions - succeeded)`
//!   minus those already active (the parallelism + remaining-completions bound);
//! * write `status.{active,succeeded,failed,complete,failed_final,finished_at}`.

use crate::apis::{Cluster, Job, Pod, PodPhase};
use crate::reconcile::Outcome;
use crate::types::{Object, ObjectMeta, OwnerReference};

/// The Job controller (holds a monotonic counter for stable pod names).
#[derive(Debug, Default)]
pub struct JobController {
    pod_seq: u64,
}

impl JobController {
    /// A fresh controller.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconcile one Job key (`"<ns>/<name>"`).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, now: u64) -> Outcome {
        let Some(job) = cluster.jobs.get(key) else {
            return Outcome::Done;
        };
        if job.meta.is_terminating() {
            return Outcome::Done;
        }

        let owned = cluster.pods.list_owned_by(&job.meta.uid);
        let succeeded = count_phase(&owned, PodPhase::Succeeded);
        let failed = count_phase(&owned, PodPhase::Failed);
        let active: Vec<&Pod> = owned.iter().filter(|p| p.is_active()).collect();
        let active_count = clamp_i32(active.len());

        // Already finished? Keep status stable (don't bump finished_at again).
        if job.status.complete || job.status.failed_final {
            return Outcome::Done;
        }

        let mut status = job.status.clone();
        status.succeeded = succeeded;
        status.failed = failed;

        if succeeded >= job.spec.completions {
            // Complete: remove any stragglers, stamp the finish time once.
            delete_active(&active, cluster);
            status.active = 0;
            status.complete = true;
            status.finished_at = Some(i64_from(now));
        } else if failed > job.spec.backoff_limit {
            delete_active(&active, cluster);
            status.active = 0;
            status.failed_final = true;
            status.finished_at = Some(i64_from(now));
        } else {
            // Create pods up to the parallelism / remaining-completions bound.
            let remaining = job.spec.completions - succeeded;
            let want = job.spec.parallelism.min(remaining);
            let to_create = (want - active_count).max(0);
            for _ in 0..to_create {
                self.create_pod(&job, cluster);
            }
            status.active = active_count + to_create;
        }

        if let Some(mut current) = cluster.jobs.get(key) {
            current.status = status;
            cluster.jobs.update(current);
        }
        Outcome::Done
    }

    fn create_pod(&mut self, job: &Job, cluster: &mut Cluster) {
        self.pod_seq += 1;
        let name = format!("{}-{}", job.meta.name, self.pod_seq);
        let mut meta = ObjectMeta::new(&name, &job.meta.namespace, "");
        meta.labels = job.spec.template.labels.clone();
        meta.owner_references = vec![OwnerReference::to("Job", &job.meta.name, &job.meta.uid)
            .controller()
            .blocking()];
        cluster.pods.create(Pod::new(meta));
    }
}

fn count_phase(pods: &[Pod], phase: PodPhase) -> i32 {
    clamp_i32(pods.iter().filter(|p| p.status.phase == phase).count())
}

fn delete_active(active: &[&Pod], cluster: &mut Cluster) {
    for p in active {
        cluster.pods.delete(&p.key());
    }
}

fn clamp_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

fn i64_from(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}
