// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the CronJob controller (`pkg/controller/cronjob`).

use cave_home_controller_manager_rs::apis::{
    Cluster, ConcurrencyPolicy, CronJob, CronJobSpec, Job, JobSpec,
};
use cave_home_controller_manager_rs::controllers::cronjob::CronJobController;
use cave_home_controller_manager_rs::reconcile::Outcome;
use cave_home_controller_manager_rs::types::Object;
use cave_home_controller_manager_rs::types::ObjectMeta;

/// 2021-01-01T00:00:00Z (a Friday) — the base for the daily-schedule cases.
const FRI: i64 = 1_609_459_200;
/// One hour, in seconds.
const HOUR: i64 = 3600;

/// A "* * * * *" (every minute) CronJob created at epoch 0, period 60s.
fn cronjob(concurrency: ConcurrencyPolicy, suspend: bool) -> CronJob {
    CronJob::new(
        ObjectMeta::new("nightly", "prod", ""),
        CronJobSpec {
            schedule: "* * * * *".to_owned(),
            concurrency,
            suspend,
            starting_deadline: None,
            ..CronJobSpec::default()
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

const PERIOD: u64 = 60;

#[test]
fn first_reconcile_starts_a_job_at_the_first_scheduled_minute() {
    let mut c = Cluster::new();
    // created_at 0, every minute → first fire is at second 60.
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    assert_eq!(ctrl.reconcile("prod/nightly", &mut c, 100), Outcome::Done);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "one job started");
    // Most recent fire at/<=100 from base 0 is second 60.
    assert_eq!(c.cronjobs.get("prod/nightly").unwrap().last_scheduled, Some(60));
}

#[test]
fn not_due_does_nothing() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100); // started, last_scheduled=60
    ctrl.reconcile("prod/nightly", &mut c, 119); // before the next minute (120)
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "no extra job before the period elapses");
}

#[test]
fn due_again_after_the_period_starts_another() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 100); // fire at 60
    ctrl.reconcile("prod/nightly", &mut c, 100 + PERIOD); // fire at 120
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
    ctrl.reconcile("prod/nightly", &mut c, 65); // run 1 at scheduled=60
    // Next fire is at 120, but we only reconcile at 200 (80s late > 10s deadline).
    ctrl.reconcile("prod/nightly", &mut c, 200);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "the late run is skipped");
    // The schedule still advances to the most-recent slot (180) so we don't keep
    // retrying the missed one.
    assert_eq!(c.cronjobs.get("prod/nightly").unwrap().last_scheduled, Some(180));
}

#[test]
fn missing_cronjob_is_a_noop() {
    let mut c = Cluster::new();
    let mut ctrl = CronJobController::new();
    assert_eq!(ctrl.reconcile("prod/ghost", &mut c, 0), Outcome::Done);
}

// --- Real cron-schedule semantics ---------------------------------------

#[test]
fn daily_schedule_fires_once_a_day_at_the_right_hour() {
    // "0 3 * * *" → 03:00 daily; created the previous midnight.
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(
        CronJob::new(
            ObjectMeta::new("backup", "prod", ""),
            CronJobSpec { schedule: "0 3 * * *".to_owned(), ..CronJobSpec::default() },
        )
        .created_at(FRI),
    );
    let mut ctrl = CronJobController::new();
    // Reconcile at 02:00 — before 03:00, nothing due.
    ctrl.reconcile("prod/backup", &mut c, (FRI + 2 * HOUR) as u64);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 0, "not due before 03:00");
    // Reconcile at 04:00 — 03:00 fired.
    ctrl.reconcile("prod/backup", &mut c, (FRI + 4 * HOUR) as u64);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "the 03:00 run started");
    assert_eq!(c.cronjobs.get("prod/backup").unwrap().last_scheduled, Some(FRI + 3 * HOUR));
}

#[test]
fn a_single_reconcile_starts_at_most_one_job_even_after_many_missed_slots() {
    // "* * * * *" but we don't reconcile for an hour: only one Job is created,
    // and last_scheduled jumps to the most recent slot (collapsing the backlog).
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(cronjob(ConcurrencyPolicy::Allow, false));
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 3600); // an hour of missed minutes
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 1, "one job, not 60");
    assert_eq!(c.cronjobs.get("prod/nightly").unwrap().last_scheduled, Some(3600));
}

#[test]
fn successful_jobs_history_limit_prunes_oldest_completed_runs() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(
        CronJob::new(
            ObjectMeta::new("nightly", "prod", ""),
            CronJobSpec {
                schedule: "* * * * *".to_owned(),
                successful_jobs_history_limit: 2,
                ..CronJobSpec::default()
            },
        ),
    );
    let owner = cj.meta().uid.clone();
    // Seed three completed Jobs with distinct finish times, owned by the CronJob.
    for (i, finish) in [10_i64, 20, 30].into_iter().enumerate() {
        let mut meta = ObjectMeta::new(&format!("nightly-old{i}"), "prod", "");
        meta.owner_references = vec![
            cave_home_controller_manager_rs::types::OwnerReference::to("CronJob", "nightly", &owner)
                .controller(),
        ];
        let mut j = Job::new(meta, JobSpec::default());
        j.status.complete = true;
        j.status.finished_at = Some(finish);
        c.jobs.create(j);
    }
    let mut ctrl = CronJobController::new();
    ctrl.reconcile("prod/nightly", &mut c, 3600);
    let completed: Vec<_> = c
        .jobs
        .list_owned_by(&owner)
        .into_iter()
        .filter(|j| j.status.complete)
        .collect();
    assert_eq!(completed.len(), 2, "history trimmed to 2 successful runs");
    // The oldest (finish 10) was the one pruned.
    assert!(
        !completed.iter().any(|j| j.status.finished_at == Some(10)),
        "the oldest completed run was deleted"
    );
}

#[test]
fn unparseable_schedule_never_fires() {
    let mut c = Cluster::new();
    let cj = c.cronjobs.create(CronJob::new(
        ObjectMeta::new("broken", "prod", ""),
        CronJobSpec { schedule: "not a schedule".to_owned(), ..CronJobSpec::default() },
    ));
    let mut ctrl = CronJobController::new();
    assert_eq!(ctrl.reconcile("prod/broken", &mut c, 99_999), Outcome::Done);
    assert_eq!(c.jobs.list_owned_by(&cj.meta().uid).len(), 0, "no run from a bad schedule");
}
