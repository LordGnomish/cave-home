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
/// The cron *expression* parsing is the scheduler crate's job; here the spec
/// carries the already-computed schedule period and the controller decides
/// *whether* to start a run given the last-scheduled time and `now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJobSpec {
    /// Schedule period in seconds (the gap between scheduled starts).
    pub period: i64,
    /// How overlapping runs are handled.
    pub concurrency: ConcurrencyPolicy,
    /// Whether the schedule is suspended.
    pub suspend: bool,
    /// Deadline (seconds) to start a missed run before giving up
    /// (`startingDeadlineSeconds`). `None` means no deadline.
    pub starting_deadline: Option<i64>,
    /// Template for the Job each run creates.
    pub job_template: JobSpec,
}

impl Default for CronJobSpec {
    fn default() -> Self {
        Self {
            period: 60,
            concurrency: ConcurrencyPolicy::Allow,
            suspend: false,
            starting_deadline: None,
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
    /// Epoch-seconds of the last scheduled run (`status.lastScheduleTime`).
    pub last_scheduled: Option<i64>,
}

impl CronJob {
    /// A `CronJob` with the given metadata and spec, never run.
    #[must_use]
    pub const fn new(meta: ObjectMeta, spec: CronJobSpec) -> Self {
        Self { meta, spec, last_scheduled: None }
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
