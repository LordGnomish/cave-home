// SPDX-License-Identifier: Apache-2.0
//! `CronJob` controller — starts a `Job` each time the schedule fires.
//!
//! Behavioural reimplementation of the documented `pkg/controller/cronjob`
//! contract, reconciling against the in-memory apiserver:
//!
//! * skip when `suspend` is set;
//! * a run is **due** when the cron period has elapsed since the last scheduled
//!   start (the first run is due immediately). The cron *expression* parsing is
//!   the scheduler's job; the spec carries the already-resolved `period`;
//! * honour `startingDeadlineSeconds`: a run started too late is skipped, but
//!   the schedule still advances so the controller does not retry the missed
//!   slot forever;
//! * apply the [`ConcurrencyPolicy`]: `Forbid` skips while a run is active,
//!   `Replace` deletes the active run first, `Allow` starts unconditionally;
//! * create the `Job` from `jobTemplate`, owned by the `CronJob`, and record
//!   `lastScheduleTime`.

use crate::apis::{Cluster, ConcurrencyPolicy, CronJob, Job};
use crate::reconcile::Outcome;
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
            return Outcome::Done;
        }

        let now = i64_from(now);
        // The instant this run was scheduled for: previous slot + period, or
        // `now` for the very first run.
        let scheduled = cj.last_scheduled.map_or(now, |last| last + cj.spec.period);
        if now < scheduled {
            return Outcome::Done; // not due yet
        }

        // Past the starting deadline? Skip the run but advance the schedule.
        if let Some(deadline) = cj.spec.starting_deadline {
            if now - scheduled > deadline {
                advance_schedule(&cj, scheduled, cluster);
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
        Outcome::Done
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

fn i64_from(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}
