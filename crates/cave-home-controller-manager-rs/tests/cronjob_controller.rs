// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the CronJob controller (`pkg/controller/cronjob`).

use cave_home_controller_manager_rs::apis::{
    ConcurrencyPolicy, CronJob, CronJobSpec, Cluster, JobSpec,
};
use cave_home_controller_manager_rs::controllers::cronjob::CronJobController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::Object;
use cave_home_controller_manager_rs::types::ObjectMeta;

const PERIOD: i64 = 60;

fn cronjob(concurrency: ConcurrencyPolicy, suspend: bool) -> CronJob {
    CronJob::new(
        ObjectMeta::new("nightly", "prod", ""),
        CronJobSpec {
            period: PERIOD,
            concurrency,
            suspend,
            starting_deadline: None,
            job_template: JobSpec::default(),
        },
    )
}

fn active_jobs(c: &Cluster, owner: &str) -> usize {
    c.jobs
        .list_owned_by(owner)
        .into_iter()
        .filter(|j| !j.status.complete && !j.status.failed_final)
        .count()
}

#[test]
fn first_reconcile_starts_a_job() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    assert_eq!(ctrl.reconcile("prod/nightly", &mut c, 100), Outcome::Done);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "one job started");
    assert_eq!(c.cronjobs.get("prod/nightly").unwrap().last_scheduled, Some(100));
}

#[test]
fn not_due_does_nothing() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100); // started
    ctrl.reconcile("prod/nightly", &mut c, 100 + PERIOD - 1); // before next tick
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "no extra job before the period elapses");
}

#[test]
fn due_again_after_the_period_starts_another() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100);
    ctrl.reconcile("prod/nightly", &mut c, 100 + PERIOD); // next tick
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 2, "second run started");
}

#[test]
fn suspended_cronjob_never_starts() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, true));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 0, "suspended: no job");
}

#[test]
fn forbid_skips_while_a_job_is_active() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Forbid, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100); // run 1 active
    ctrl.reconcile("prod/nightly", &mut c, 100 + PERIOD); // due, but run 1 still active
    assert_eq!(active_jobs(&c, &cj.meta().uid), 1, "Forbid does not start a concurrent run");
}

#[test]
fn replace_deletes_the_active_run_then_starts_a_new_one() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Replace, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100);
    let first: Vec<_> = c.jobs.list_owned_by(&cj.meta().uid).iter().map(|j| j.meta().uid.clone()).collect();
    ctrl.reconcile("prod/nightly", &mut c, 100 + PERIOD);
    let now_jobs: Vec<_> = c.jobs.list_owned_by(&cj.meta().uid).iter().map(|j| j.meta().uid.clone()).collect();
    assert_eq!(active_jobs(&c, &cj.meta().uid), 1, "exactly one active run");
    assert!(!now_jobs.contains(&first[0]), "the previous run was replaced");
}

#[test]
fn allow_starts_even_with_an_active_run() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100);
    ctrl.reconcile("prod/nightly", &mut c, 100 + PERIOD);
    assert_eq!(active_jobs(&c, &cj.meta().uid), 2, "Allow permits concurrent runs");
}

#[test]
fn missed_run_past_the_starting_deadline_is_skipped() {
    let mut c = Cluster::new();
    let mut spec = cronjob(ConcurrencyPolicy::Allow, false);
    spec.spec.starting_deadline = Some(10);
    let cj = c.cronjobs.create(spec);
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100); // run 1 at 100
    // Next tick is due at 160, but we only reconcile at 200 (40s late > 10s deadline).
    ctrl.reconcile("prod/nightly", &mut c, 200);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "the late run is skipped");
    // The schedule still advances so we don't keep retrying the missed slot.
    assert_eq!(c.cronjobs.get("prod/nightly").unwrap().last_scheduled, Some(160));
}

#[test]
fn missing_cronjob_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = CronJobController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
}
