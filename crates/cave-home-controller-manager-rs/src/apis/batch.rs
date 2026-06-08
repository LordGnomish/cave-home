// SPDX-License-Identifier: Apache-2.0
//! `batch/v1` object subset: [`Job`] and [`CronJob`].

use crate::apis::{LabelSelector, PodTemplateSpec};
use crate::types::{Object, ObjectMeta};

/// Desired state of a Job (`batch/v1` `JobSpec` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    /// Number of successful completions required. `1` if `None` upstream;
    /// modelled here as an explicit count.
    pub completions: i32,
    /// Max pods running at once.
    pub parallelism: i32,
    /// Retries before the Job is marked failed (`backoffLimit`).
    pub backoff_limit: i32,
    /// Selector for owned pods.
    pub selector: LabelSelector,
    /// Pod template.
    pub template: PodTemplateSpec,
}

impl Default for JobSpec {
    fn default() -> Self {
        Self {
            completions: 1,
            parallelism: 1,
            backoff_limit: 6,
            selector: LabelSelector::new(),
            template: PodTemplateSpec::default(),
        }
    }
}

/// Observed state of a Job (`batch/v1` `JobStatus` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JobStatus {
    /// Pods that succeeded.
    pub succeeded: i32,
    /// Pods that failed.
    pub failed: i32,
    /// Pods currently active.
    pub active: i32,
    /// Whether the Job has completed (all completions reached).
    pub complete: bool,
    /// Whether the Job has failed (exceeded backoff limit).
    pub failed_final: bool,
    /// Epoch-seconds the Job finished, if it has (`completionTime`).
    pub finished_at: Option<i64>,
}

/// A Job (`batch/v1` `Job` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Desired state.
    pub spec: JobSpec,
    /// Observed state.
    pub status: JobStatus,
}

impl Job {
    /// A Job with the given metadata and spec, empty status.
    #[must_use]
    pub fn new(meta: ObjectMeta, spec: JobSpec) -> Self {
        Self { meta, spec, status: JobStatus::default() }
    }
}

impl Object for Job {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// How a `CronJob` handles an overlapping run (`batch/v1` `ConcurrencyPolicy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum ConcurrencyPolicy {
    /// Allow concurrent runs.
    #[default]
    Allow,
    /// Skip a run if the previous is still active.
    Forbid,
    /// Replace the active run with the new one.
    Replace,
}


/// Desired state of a `CronJob` (`batch/v1` `CronJobSpec` subset).
///
/// `schedule` is a real 5-field cron expression
/// ([`crate::schedule::CronSchedule`]); the controller parses it and computes
/// the most-recent due time from `last_scheduled` and the caller-supplied
/// `now`, exactly as `pkg/controller/cronjob`'s `getNextScheduleTime` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJobSpec {
    /// The cron expression (`spec.schedule`), e.g. `"0 3 * * *"`.
    pub schedule: String,
    /// How overlapping runs are handled.
    pub concurrency: ConcurrencyPolicy,
    /// Whether the schedule is suspended.
    pub suspend: bool,
    /// Deadline (seconds) to start a missed run before giving up
    /// (`startingDeadlineSeconds`). `None` means no deadline.
    pub starting_deadline: Option<i64>,
    /// How many completed Jobs to retain (`successfulJobsHistoryLimit`).
    pub successful_jobs_history_limit: usize,
    /// How many failed Jobs to retain (`failedJobsHistoryLimit`).
    pub failed_jobs_history_limit: usize,
    /// Template for the Job each run creates.
    pub job_template: JobSpec,
}

impl Default for CronJobSpec {
    fn default() -> Self {
        Self {
            schedule: "* * * * *".to_owned(),
            concurrency: ConcurrencyPolicy::Allow,
            suspend: false,
            starting_deadline: None,
            // Upstream defaults: keep 3 successful, 1 failed.
            successful_jobs_history_limit: 3,
            failed_jobs_history_limit: 1,
            job_template: JobSpec::default(),
        }
    }
}

/// A `CronJob` (`batch/v1` `CronJob` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJob {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Desired state.
    pub spec: CronJobSpec,
    /// Epoch-seconds the `CronJob` was created (`metadata.creationTimestamp`),
    /// the base from which the first scheduled time is computed before any run.
    pub created_at: i64,
    /// Epoch-seconds of the last scheduled run (`status.lastScheduleTime`).
    pub last_scheduled: Option<i64>,
}

impl CronJob {
    /// A `CronJob` with the given metadata and spec, created at epoch second 0
    /// and never run. Use [`CronJob::created_at`] for a non-zero base time.
    #[must_use]
    pub const fn new(meta: ObjectMeta, spec: CronJobSpec) -> Self {
        Self { meta, spec, created_at: 0, last_scheduled: None }
    }

    /// Set the creation timestamp (the schedule base before the first run).
    #[must_use]
    pub const fn created_at(mut self, epoch: i64) -> Self {
        self.created_at = epoch;
        self
    }
}

impl Object for CronJob {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}
