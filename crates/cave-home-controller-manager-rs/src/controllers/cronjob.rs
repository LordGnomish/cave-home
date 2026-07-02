// SPDX-License-Identifier: Apache-2.0
//! `CronJob` controller — starts a `Job` each time the cron schedule fires.
//!
//! Behavioural reimplementation of the documented `pkg/controller/cronjob`
//! contract, reconciling against the in-memory apiserver:
//!
//! * skip when `suspend` is set;
//! * parse the real cron `schedule` ([`crate::schedule::CronSchedule`]) and find
//!   the **most recent** scheduled time at or before `now` that is strictly
//!   after the base (`last_scheduled`, else `created_at`) — the
//!   `getNextScheduleTime` / `getMostRecentScheduleTime` contract. If there is
//!   none, the run is not due;
//! * honour `startingDeadlineSeconds`: a run whose scheduled instant is older
//!   than the deadline is skipped, but the schedule still advances so the
//!   controller does not retry the missed slot forever;
//! * apply the [`ConcurrencyPolicy`]: `Forbid` skips while a run is active,
//!   `Replace` deletes the active run first, `Allow` starts unconditionally;
//! * create the `Job` from `jobTemplate`, owned by the `CronJob`, record
//!   `lastScheduleTime`, then prune finished Jobs down to
//!   `successfulJobsHistoryLimit` / `failedJobsHistoryLimit`.

use crate::apis::{Cluster, ConcurrencyPolicy, CronJob, Job};
use crate::reconcile::Outcome;
use crate::schedule::CronSchedule;
use crate::types::{Object, ObjectMeta, OwnerReference};

/// The `CronJob` controller.
#[derive(Debug, Default)]
pub struct CronJobController;

impl CronJobController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one `CronJob` key (`"<ns>/<name>"`).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, now: u64) -> Outcome {
        let Some(cj) = cluster.cronjobs.get(key) else {
            return Outcome::Done;
        };
        if cj.meta.is_terminating() || cj.spec.suspend {
            // A suspended CronJob still prunes its history (upstream does the
            // sync's cleanup before returning), but never starts new runs.
            prune_history(&cj, cluster);
            return Outcome::Done;
        }

        let Ok(schedule) = CronSchedule::parse(&cj.spec.schedule) else {
            // An unparseable schedule never fires (upstream logs and requeues).
            return Outcome::Done;
        };

        let now = i64_from(now);
        // Base for the schedule walk: the last scheduled time, else creation.
        let base = cj.last_scheduled.unwrap_or(cj.created_at);
        // The most recent scheduled instant in (base, now].
        let Some(scheduled) = most_recent_schedule(&schedule, base, now) else {
            prune_history(&cj, cluster);
            return Outcome::Done; // not due yet
        };

        // Past the starting deadline? Skip the run but advance the schedule.
        if let Some(deadline) = cj.spec.starting_deadline {
            if now - scheduled > deadline {
                advance_schedule(&cj, scheduled, cluster);
                prune_history(&cj, cluster);
                return Outcome::Done;
            }
        }

        // Concurrency policy.
        let active: Vec<Job> = cluster
            .jobs
            .list_owned_by(&cj.meta.uid)
            .into_iter()
            .filter(|j| !j.status.complete && !j.status.failed_final)
            .collect();
        match cj.spec.concurrency {
            ConcurrencyPolicy::Forbid if !active.is_empty() => {
                // Leave the schedule where it is and retry next reconcile.
                prune_history(&cj, cluster);
                return Outcome::Done;
            }
            ConcurrencyPolicy::Replace => {
                for j in &active {
                    cluster.jobs.delete(&j.key());
                }
            }
            ConcurrencyPolicy::Forbid | ConcurrencyPolicy::Allow => {}
        }

        start_job(&cj, scheduled, cluster);
        advance_schedule(&cj, scheduled, cluster);
        prune_history(&cj, cluster);
        Outcome::Done
    }
}

/// The most recent scheduled instant in the half-open interval `(base, now]`,
/// or `None` if the schedule has not fired since `base`.
///
/// Walks forward from `base` (each `next_after` returns a strictly-later fire),
/// keeping the last one that is `<= now`. Mirrors upstream
/// `getMostRecentScheduleTime`, which likewise iterates the schedule rather
/// than inverting it.
fn most_recent_schedule(schedule: &CronSchedule, base: i64, now: i64) -> Option<i64> {
    let mut t = schedule.next_after(base);
    if t > now {
        return None;
    }
    loop {
        let next = schedule.next_after(t);
        if next > now {
            return Some(t);
        }
        t = next;
    }
}

/// Create the `Job` for this scheduled run, owned by the `CronJob`.
fn start_job(cj: &CronJob, scheduled: i64, cluster: &mut Cluster) {
    let mut meta = ObjectMeta::new(&format!("{}-{scheduled}", cj.meta.name), &cj.meta.namespace, "");
    meta.owner_references = vec![OwnerReference::to("CronJob", &cj.meta.name, &cj.meta.uid)
        .controller()
        .blocking()];
    cluster.jobs.create(Job::new(meta, cj.spec.job_template.clone()));
}

/// Record the scheduled time as the `CronJob`'s last run.
fn advance_schedule(cj: &CronJob, scheduled: i64, cluster: &mut Cluster) {
    if let Some(mut current) = cluster.cronjobs.get(&cj.key()) {
        current.last_scheduled = Some(scheduled);
        cluster.cronjobs.update(current);
    }
}

/// Trim finished owned Jobs to the configured history limits, deleting the
/// oldest first (oldest = earliest `finished_at`). Active Jobs are never
/// touched. Mirrors `cleanupFinishedJobs`.
fn prune_history(cj: &CronJob, cluster: &mut Cluster) {
    let owned = cluster.jobs.list_owned_by(&cj.meta.uid);
    prune_one(
        owned.iter().filter(|j| j.status.complete).collect(),
        cj.spec.successful_jobs_history_limit,
        cluster,
    );
    prune_one(
        owned.iter().filter(|j| j.status.failed_final).collect(),
        cj.spec.failed_jobs_history_limit,
        cluster,
    );
}

/// Delete the oldest finished Jobs beyond `limit` from one history bucket.
fn prune_one(mut jobs: Vec<&Job>, limit: usize, cluster: &mut Cluster) {
    if jobs.len() <= limit {
        return;
    }
    // Oldest first: by finish time, then by name for a stable tiebreak.
    jobs.sort_by(|a, b| {
        a.status
            .finished_at
            .cmp(&b.status.finished_at)
            .then_with(|| a.meta.name.cmp(&b.meta.name))
    });
    let surplus = jobs.len() - limit;
    let to_delete: Vec<String> = jobs.iter().take(surplus).map(|j| j.key()).collect();
    for key in to_delete {
        cluster.jobs.delete(&key);
    }
}

fn i64_from(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}
