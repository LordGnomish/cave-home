//! `cave-home-wellness` — privacy-first personal-wellness intelligence (ADR-025).
//!
//! This crate is the **brain** that turns everyday wellness numbers — steps,
//! active minutes, sleep, resting heart rate, body weight — into gentle,
//! grandma-friendly nudges a household can act on. It computes goal progress,
//! sorts each metric into a small plain-language band with encouraging advice in
//! EN / DE / TR, and reads simple weekly trends and goal streaks.
//!
//! # Not medical advice
//!
//! Everything here is **wellness framing, not medical diagnosis.** The bands and
//! copy are intentionally warm and free of clinical / alarming language
//! (ADR-025, Charter §6.3): "you slept well", "nice walk" — never a diagnosis.
//! Anyone wanting a medical opinion should see a clinician; cave-home only nudges.
//!
//! # Privacy
//!
//! Health data is the most sensitive data in the home (Charter §6/§7/§8/§9).
//! This engine is **pure and on-device**: it reads no clock, opens no socket,
//! and touches no storage. Day ordinals are supplied by the caller. **Wellness
//! data is never uploaded to any cloud** — that boundary is a *permanent*
//! disposition in `parity.manifest.toml`, not a phase deferral.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`metric`] — validated metric value objects + the [`metric::DailyMetrics`]
//!   aggregate.
//! - [`band`] — resting-HR / sleep / step-activity bands with EN/DE/TR names and
//!   gentle, non-clinical advice (Charter §6.3, ADR-007).
//! - [`goal`] — the goal engine: [`goal::Goal`] + [`goal::progress`].
//! - [`trend`] — pure Improving / Steady / Declining trend classifier.
//! - [`streak`] — consecutive goal-met-day counting.
//!
//! The **wearable / health adapters** (Apple Health, Google Fit, Fitbit,
//! Withings, Garmin, BLE heart-rate / scale), the persistent on-device history
//! store, and cave-home-core integration are network / BLE / storage-bound and
//! deferred to phase-1b — each is enumerated in `parity.manifest.toml`
//! `[[unmapped]]` with an ADR-025 disposition. They map their wire values onto
//! [`metric`] types and then reuse this engine unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_wellness::{
//!     ActivityBand, DailyMetrics, Goal, HeartRate, Lang, SleepDuration,
//!     Steps, ActiveMinutes, progress,
//! };
//!
//! // Yesterday: a nicely active day with a good night's sleep.
//! let today = DailyMetrics::new(
//!     /* caller-assigned day ordinal */ 42,
//!     Steps::new(8_200)?,
//!     ActiveMinutes::new(35)?,
//!     SleepDuration::from_hours(8)?,
//!     HeartRate::new(58)?,
//! );
//!
//! // Did we hit a 10,000-step goal? Not quite — but it was still an active day.
//! let p = progress(&today, Goal::daily_steps(10_000));
//! assert_eq!(p.percent, 82);
//! assert!(!p.met);
//!
//! let band = ActivityBand::from_steps(today.steps.get());
//! assert_eq!(band, ActivityBand::Active);
//! println!("{}: {}", band.name(Lang::En), band.advice(Lang::En));
//! # Ok::<(), cave_home_wellness::MetricError>(())
//! ```

pub mod band;
pub mod goal;
pub mod label;
pub mod metric;
pub mod streak;
pub mod trend;

pub use band::{ActivityBand, RestingHrBand, SleepBand};
pub use goal::{progress, Goal, GoalMetric, GoalProgress, Period};
pub use label::Lang;
pub use metric::{
    ActiveMinutes, BodyWeight, DailyMetrics, HeartRate, MetricError, SleepDuration, Steps,
};
pub use streak::{current_streak, longest_streak};
pub use trend::{classify_trend, Trend};
