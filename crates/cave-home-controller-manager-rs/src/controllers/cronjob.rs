// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/cronjob/cronjob_controllerv2.go
//         pkg/controller/cronjob/utils.go
//
//! CronJobController.
//!
//! Two responsibilities:
//!   1) `getNextScheduleTime` — parse the cron expression and decide if a new
//!      Job should be spawned at the requested `now`.
//!   2) Apply the concurrency policy + history limit + spawn / GC.
//!
//! Phase 2 ships its own minimal cron parser (numeric values + `*` only;
//! upstream supports `,`/`-`/`/` and named months — partly mapped, the
//! ranges + step form are `[[unmapped]]`).

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::types::{
    new_controller_ref, ConcurrencyPolicy, CronJob, Job, JobSpec, KubeResource, ObjectMeta,
};

/// Parsed 5-field cron expression. Each field is either `None` (`*`, "any") or
/// a list of allowed values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CronSchedule {
    pub minutes: Option<Vec<u8>>,
    pub hours: Option<Vec<u8>>,
    pub days_of_month: Option<Vec<u8>>,
    pub months: Option<Vec<u8>>,
    pub days_of_week: Option<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum CronParseError {
    #[error("schedule must have exactly 5 fields, got {0}")]
    WrongArity(usize),
    #[error("invalid field {field}: {value}")]
    InvalidField { field: &'static str, value: String },
}

impl CronSchedule {
    /// Parse a cron expression. Phase 2 supports `*`, single integers, and
    /// comma-separated lists (`1,5,10`).
    pub fn parse(expr: &str) -> Result<Self, CronParseError> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(CronParseError::WrongArity(parts.len()));
        }
        Ok(Self {
            minutes: parse_field("minute", parts[0], 0, 59)?,
            hours: parse_field("hour", parts[1], 0, 23)?,
            days_of_month: parse_field("dom", parts[2], 1, 31)?,
            months: parse_field("month", parts[3], 1, 12)?,
            days_of_week: parse_field("dow", parts[4], 0, 6)?,
        })
    }

    fn matches(&self, cal: &Calendar) -> bool {
        Self::field_match(&self.minutes, cal.minute)
            && Self::field_match(&self.hours, cal.hour)
            && Self::field_match(&self.days_of_month, cal.day)
            && Self::field_match(&self.months, cal.month)
            && Self::field_match(&self.days_of_week, cal.day_of_week)
    }

    fn field_match(field: &Option<Vec<u8>>, v: u8) -> bool {
        match field {
            None => true,
            Some(allowed) => allowed.contains(&v),
        }
    }

    /// Find the next minute-boundary time strictly greater than
    /// `start_unix_secs` whose calendar fields match.
    #[must_use]
    pub fn next_after(&self, start_unix_secs: u64) -> Option<u64> {
        // Round UP to the next minute boundary.
        let mut t = start_unix_secs - (start_unix_secs % 60) + 60;
        let limit = start_unix_secs + 4 * 366 * 24 * 60 * 60;
        while t <= limit {
            let cal = unix_to_calendar(t);
            if self.matches(&cal) {
                return Some(t);
            }
            t += 60;
        }
        None
    }

    /// Find the most recent minute-boundary time `<= end_unix_secs` that
    /// matches the schedule. Mirrors `getRecentUnmetScheduleTime` (modulo
    /// the time-window logic upstream uses for "missed schedule"
    /// detection — Phase 2 keeps it simple).
    #[must_use]
    pub fn previous_at_or_before(&self, end_unix_secs: u64) -> Option<u64> {
        // Round DOWN to the current minute boundary.
        let mut t = end_unix_secs - (end_unix_secs % 60);
        // Bound the scan to 366 days back — anything older is irrelevant for
        // a healthy controller.
        let floor = end_unix_secs.saturating_sub(366 * 24 * 60 * 60);
        while t >= floor {
            let cal = unix_to_calendar(t);
            if self.matches(&cal) {
                return Some(t);
            }
            if t < 60 {
                break;
            }
            t -= 60;
        }
        None
    }
}

fn parse_field(
    name: &'static str,
    raw: &str,
    min: u8,
    max: u8,
) -> Result<Option<Vec<u8>>, CronParseError> {
    if raw == "*" {
        return Ok(None);
    }
    let mut out = Vec::new();
    for piece in raw.split(',') {
        let v: u8 = piece.parse().map_err(|_| CronParseError::InvalidField {
            field: name,
            value: piece.to_string(),
        })?;
        if v < min || v > max {
            return Err(CronParseError::InvalidField {
                field: name,
                value: piece.to_string(),
            });
        }
        out.push(v);
    }
    Ok(Some(out))
}

#[derive(Clone, Copy, Debug)]
struct Calendar {
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    day_of_week: u8,
}

/// Tiny gregorian-calendar conversion (no time zones, leap years honoured).
fn unix_to_calendar(unix_secs: u64) -> Calendar {
    let days = unix_secs / 86_400;
    let seconds_of_day = unix_secs % 86_400;
    let hour = (seconds_of_day / 3600) as u8;
    let minute = ((seconds_of_day % 3600) / 60) as u8;
    let day_of_week = ((days + 4) % 7) as u8; // 1970-01-01 = Thursday (4).
    let (_year, month, day) = days_to_ymd(days as i64);
    Calendar {
        minute,
        hour,
        day: day as u8,
        month: month as u8,
        day_of_week,
    }
}

fn days_to_ymd(mut days: i64) -> (i64, i64, i64) {
    let mut year: i64 = 1970;
    loop {
        let dy = if is_leap_year(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let month_lengths: [i64; 12] = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: i64 = 1;
    for &m in &month_lengths {
        if days < m {
            break;
        }
        days -= m;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ---------------------------------------------------------------------------
// sync_cron_job
// ---------------------------------------------------------------------------

/// Inject a clock so tests don't depend on wall time.
pub trait Clock: Send + Sync {
    fn now_unix_secs(&self) -> u64;
}

/// Real-time clock (production).
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_unix_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// One reconcile pass.
///
/// Mirrors `ControllerV2.syncCronJob`.
pub async fn sync_cron_job<C: ControllerApiClient>(
    client: &C,
    clock: &dyn Clock,
    namespace: &str,
    name: &str,
) -> ApiResult<crate::types::CronJobStatus> {
    let mut cj: CronJob = client.get(Some(namespace), name).await?;
    let schedule = CronSchedule::parse(&cj.spec.schedule)
        .map_err(|e| crate::api_client::ApiError::Invalid(format!("schedule: {e}")))?;

    let now = clock.now_unix_secs();
    // Find the most recent firing time at-or-before `now`. If we have never
    // fired (no last_schedule_time) or that timestamp is strictly greater
    // than the recorded last, we have an unmet schedule — fire it.
    let last = cj.status.last_schedule_time_ms.map(|ms| ms / 1000);
    let next = schedule.previous_at_or_before(now);
    let should_fire = match (next, last) {
        (Some(t), Some(l)) => t > l,
        (Some(_), None) => true,
        (None, _) => false,
    };

    // Refresh active jobs (drop completed).
    let owner_uid = cj.uid().clone();
    let all_jobs: Vec<Job> = client.list(Some(namespace), None).await?;
    let owned_jobs: Vec<Job> = all_jobs
        .into_iter()
        .filter(|j| crate::types::is_controlled_by(j.meta(), &owner_uid))
        .collect();
    let active: Vec<&Job> = owned_jobs.iter().filter(|j| !j.status.completed).collect();

    if should_fire {
        let allow_fire = match cj.spec.concurrency_policy {
            ConcurrencyPolicy::Allow => true,
            ConcurrencyPolicy::Forbid => active.is_empty(),
            ConcurrencyPolicy::Replace => {
                for j in &active {
                    client.delete("Job", Some(namespace), j.name()).await?;
                }
                true
            }
        };
        if allow_fire {
            if let Some(t) = next {
                let job = build_job(&cj, t);
                client.create(Some(namespace), job).await?;
                cj.status.last_schedule_time_ms = Some(t * 1000);
                cj.status.active_jobs.push(format!("{}-{t}", cj.name()));
            }
        } else if let Some(t) = next {
            // Even when Forbid suppresses the fire, upstream still advances
            // last_schedule_time so that next sweep doesn't double-skip past
            // long-running active jobs.
            cj.status.last_schedule_time_ms = Some(t * 1000);
        }
    }

    gc_history(client, namespace, &owned_jobs, cj.spec.successful_jobs_history_limit, true).await?;
    gc_history(client, namespace, &owned_jobs, cj.spec.failed_jobs_history_limit, false).await?;

    let updated = client.update(Some(namespace), cj).await?;
    Ok(updated.status)
}

async fn gc_history<C: ControllerApiClient>(
    client: &C,
    namespace: &str,
    owned_jobs: &[Job],
    limit: i32,
    keep_successful: bool,
) -> ApiResult<()> {
    if limit < 0 {
        return Ok(());
    }
    let mut filtered: Vec<&Job> = owned_jobs
        .iter()
        .filter(|j| j.status.completed)
        .filter(|j| {
            if keep_successful {
                j.status.failed == 0
            } else {
                j.status.failed > 0
            }
        })
        .collect();
    filtered.sort_by_key(|j| j.meta().resource_version);
    let excess = filtered.len() as i32 - limit;
    if excess <= 0 {
        return Ok(());
    }
    for j in filtered.iter().take(excess as usize) {
        client.delete("Job", Some(namespace), j.name()).await?;
    }
    Ok(())
}

fn build_job(cj: &CronJob, schedule_secs: u64) -> Job {
    let mut meta = ObjectMeta {
        name: format!("{}-{schedule_secs}", cj.name()),
        namespace: cj.namespace().into(),
        ..Default::default()
    };
    meta.owner_references.push(new_controller_ref(cj, "batch/v1"));
    Job {
        metadata: meta,
        spec: JobSpec {
            parallelism: cj.spec.job_template.parallelism.max(1),
            completions: cj.spec.job_template.completions.max(1),
            backoff_limit: cj.spec.job_template.backoff_limit,
            selector: cj.spec.job_template.selector.clone(),
            template: cj.spec.job_template.template.clone(),
        },
        status: Default::default(),
    }
}

pub struct CronJobController<C: ControllerApiClient> {
    client: Arc<C>,
    clock: Arc<dyn Clock>,
}

impl<C: ControllerApiClient> CronJobController<C> {
    pub fn new(client: Arc<C>, clock: Arc<dyn Clock>) -> Self {
        Self { client, clock }
    }

    pub async fn reconcile(&self, key: &str) -> ApiResult<()> {
        let (ns, name) = crate::informer::split_meta_namespace_key(key);
        sync_cron_job(self.client.as_ref(), self.clock.as_ref(), &ns, &name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{CronJobSpec, JobSpec, LabelSelector, PodTemplateSpec};

    struct FixedClock(std::sync::Mutex<u64>);
    impl Clock for FixedClock {
        fn now_unix_secs(&self) -> u64 {
            *self.0.lock().unwrap()
        }
    }
    impl FixedClock {
        fn at(t: u64) -> Self {
            Self(std::sync::Mutex::new(t))
        }
        fn advance(&self, by: u64) {
            *self.0.lock().unwrap() += by;
        }
    }

    fn make_cj(name: &str, schedule: &str, policy: ConcurrencyPolicy) -> CronJob {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("cronjob".into(), name.into());
        CronJob {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: "default".into(),
                ..Default::default()
            },
            spec: CronJobSpec {
                schedule: schedule.into(),
                concurrency_policy: policy,
                job_template: JobSpec {
                    parallelism: 1,
                    completions: 1,
                    backoff_limit: 6,
                    selector: sel,
                    template: PodTemplateSpec::default(),
                },
                successful_jobs_history_limit: 3,
                failed_jobs_history_limit: 1,
            },
            ..Default::default()
        }
    }

    #[test]
    fn parses_every_minute() {
        let s = CronSchedule::parse("* * * * *").unwrap();
        assert!(s.minutes.is_none());
    }

    #[test]
    fn parses_specific_minute() {
        let s = CronSchedule::parse("0 * * * *").unwrap();
        assert_eq!(s.minutes, Some(vec![0]));
    }

    #[test]
    fn rejects_wrong_arity() {
        assert!(matches!(
            CronSchedule::parse("* * * *"),
            Err(CronParseError::WrongArity(4))
        ));
    }

    #[test]
    fn next_after_finds_minute_boundary() {
        let s = CronSchedule::parse("* * * * *").unwrap();
        let next = s.next_after(30).unwrap();
        assert_eq!(next, 60);
    }

    #[test]
    fn next_after_finds_specific_hour() {
        let s = CronSchedule::parse("0 3 * * *").unwrap();
        assert_eq!(s.next_after(0).unwrap(), 3 * 3600);
    }

    #[tokio::test]
    async fn fires_when_schedule_is_due() {
        let c = InMemoryApiClient::new();
        c.seed(
            Some("default"),
            make_cj("hourly", "0 * * * *", ConcurrencyPolicy::Allow),
        );
        let clock = FixedClock::at(4 * 3600 + 30 * 60);
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        assert_eq!(c.count("Job"), 1);
    }

    #[tokio::test]
    async fn does_not_double_fire_within_same_minute() {
        let c = InMemoryApiClient::new();
        c.seed(
            Some("default"),
            make_cj("hourly", "0 * * * *", ConcurrencyPolicy::Allow),
        );
        let clock = FixedClock::at(4 * 3600 + 30 * 60);
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        assert_eq!(c.count("Job"), 1);
    }

    #[tokio::test]
    async fn forbid_policy_skips_when_active_jobs_exist() {
        let c = InMemoryApiClient::new();
        c.seed(
            Some("default"),
            make_cj("hourly", "0 * * * *", ConcurrencyPolicy::Forbid),
        );
        let clock = FixedClock::at(4 * 3600 + 30 * 60);
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        clock.advance(3600);
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        assert_eq!(c.count("Job"), 1);
    }

    #[tokio::test]
    async fn replace_policy_deletes_active_then_fires() {
        let c = InMemoryApiClient::new();
        c.seed(
            Some("default"),
            make_cj("hourly", "0 * * * *", ConcurrencyPolicy::Replace),
        );
        let clock = FixedClock::at(4 * 3600 + 30 * 60);
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        clock.advance(3600);
        sync_cron_job(&c, &clock, "default", "hourly").await.unwrap();
        assert_eq!(c.count("Job"), 1);
    }
}
