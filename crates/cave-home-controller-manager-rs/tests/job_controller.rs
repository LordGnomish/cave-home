// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the Job controller (`pkg/controller/job` contract).

use cave_home_controller_manager_rs::apis::{Cluster, Job, JobSpec, PodPhase, PodTemplateSpec};
use cave_home_controller_manager_rs::controllers::job::JobController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::{Object, ObjectMeta};

fn sel() -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("job-name".to_owned(), "batch".to_owned());
    m
}

fn job(completions: i32, parallelism: i32, backoff: i32) -> Job {
    Job::new(
        ObjectMeta::new("batch", "prod", ""),
        JobSpec {
            completions,
            parallelism,
            backoff_limit: backoff,
            selector: sel(),
            template: PodTemplateSpec::with_labels(&[("job-name", "batch")]),
        },
    )
}

fn finish_pods(c: &mut Cluster, job_uid: &str, phase: PodPhase, count: usize) {
    let pods: Vec<_> = c.pods.list_owned_by(job_uid).into_iter().take(count).collect();
    for mut p in pods {
        p.status.phase = phase;
        c.pods.update(p);
    }
}

#[test]
fn job_creates_up_to_parallelism_pods() {
    let mut c = Cluster::new();
    let j = c.jobs.create(job(3, 2, 6));
    let mut ctrl = JobController::new();
    assert_eq!(ctrl.reconcile("prod/batch", &mut c, 0), Outcome::Done);
    let pods = c.pods.list_owned_by(&j.meta().uid);
    assert_eq!(pods.len(), 2, "bounded by parallelism");
    assert_eq!(c.jobs.get("prod/batch").unwrap().status.active, 2);
}

#[test]
fn job_does_not_exceed_remaining_completions() {
    let mut c = Cluster::new();
    let j = c.jobs.create(job(3, 5, 6)); // parallelism 5 > completions 3
    let mut ctrl = JobController::new();
    ctrl.reconcile("prod/batch", &mut c, 0);
    assert_eq!(c.pods.list_owned_by(&j.meta().uid).len(), 3, "capped at completions");
}

#[test]
fn job_backfills_after_successes() {
    let mut c = Cluster::new();
    let j = c.jobs.create(job(3, 2, 6));
    let mut ctrl = JobController::new();
    ctrl.reconcile("prod/batch", &mut c, 0); // 2 active
    finish_pods(&mut c, &j.meta().uid, PodPhase::Succeeded, 2);
    // Remove the succeeded pods from "active" by leaving them terminal; reconcile.
    ctrl.reconcile("prod/batch", &mut c, 1);
    let active = c.pods.list_owned_by(&j.meta().uid).into_iter().filter(|p| p.is_active()).count();
    assert_eq!(active, 1, "one more pod to reach 3 completions");
    assert_eq!(c.jobs.get("prod/batch").unwrap().status.succeeded, 2);
}

#[test]
fn job_completes_when_completions_reached() {
    let mut c = Cluster::new();
    let j = c.jobs.create(job(2, 2, 6));
    let mut ctrl = JobController::new();
    ctrl.reconcile("prod/batch", &mut c, 0);
    finish_pods(&mut c, &j.meta().uid, PodPhase::Succeeded, 2);
    ctrl.reconcile("prod/batch", &mut c, 100);
    let done = c.jobs.get("prod/batch").unwrap();
    assert!(done.status.complete, "job marked complete");
    assert_eq!(done.status.finished_at, Some(100));
    assert_eq!(done.status.succeeded, 2);
}

#[test]
fn job_fails_after_backoff_limit() {
    let mut c = Cluster::new();
    let j = c.jobs.create(job(3, 3, 2)); // backoff_limit 2
    let mut ctrl = JobController::new();
    ctrl.reconcile("prod/batch", &mut c, 0); // 3 active
    finish_pods(&mut c, &j.meta().uid, PodPhase::Failed, 3); // 3 failures > 2
    ctrl.reconcile("prod/batch", &mut c, 50);
    let failed = c.jobs.get("prod/batch").unwrap();
    assert!(failed.status.failed_final, "job marked failed");
    assert_eq!(failed.status.finished_at, Some(50));
    // No active pods remain on a failed job.
    assert_eq!(c.pods.list_owned_by(&j.meta().uid).iter().filter(|p| p.is_active()).count(), 0);
}

#[test]
fn completed_job_is_idempotent() {
    let mut c = Cluster::new();
    let j = c.jobs.create(job(1, 1, 6));
    let mut ctrl = JobController::new();
    ctrl.reconcile("prod/batch", &mut c, 0);
    finish_pods(&mut c, &j.meta().uid, PodPhase::Succeeded, 1);
    ctrl.reconcile("prod/batch", &mut c, 10);
    let before = c.jobs.get("prod/batch").unwrap();
    ctrl.reconcile("prod/batch", &mut c, 20);
    let after = c.jobs.get("prod/batch").unwrap();
    assert_eq!(before.status.finished_at, after.status.finished_at, "finish time not bumped again");
}

#[test]
fn missing_job_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = JobController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
}
