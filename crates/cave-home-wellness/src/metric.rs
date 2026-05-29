//! Validated wellness metric value objects.
//!
//! Every metric is constructed through a fallible constructor that rejects
//! impossible values up front, so the band / goal / trend logic downstream
//! never has to defend against nonsense (negative steps, a 400 bpm pulse, a
//! 36-hour "day" of sleep). These are vendor-neutral: a phase-1b wearable
//! adapter (Apple Health, Google Fit, Fitbit, Withings, Garmin — all deferred,
//! see the parity manifest) maps its wire values onto these types and the rest
//! of the engine works off this model alone.
//!
//! Sanity bounds are deliberately generous "is this physically plausible for a
//! living person" guards, **not** medical judgements. The wellness bands in
//! [`crate::band`] do the gentle, non-clinical interpretation.

/// Why a metric value object could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricError {
    /// A floating-point input was `NaN` or infinite.
    NotFinite,
    /// The value is below the plausible range for a living person.
    TooLow,
    /// The value is above the plausible range for a living person.
    TooHigh,
}

impl core::fmt::Display for MetricError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFinite => f.write_str("value is not a finite number"),
            Self::TooLow => f.write_str("value is below the plausible range"),
            Self::TooHigh => f.write_str("value is above the plausible range"),
        }
    }
}

impl std::error::Error for MetricError {}

/// A step count for a period (typically one day).
///
/// Upper bound is a generous plausibility guard: well above any ultra-marathon
/// day, far below an obvious sensor glitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Steps(u32);

impl Steps {
    /// Largest step count treated as a real human day rather than a glitch.
    pub const MAX: u32 = 200_000;

    /// Construct a validated step count.
    ///
    /// # Errors
    /// Returns [`MetricError::TooHigh`] above [`Steps::MAX`].
    pub const fn new(count: u32) -> Result<Self, MetricError> {
        if count > Self::MAX {
            return Err(MetricError::TooHigh);
        }
        Ok(Self(count))
    }

    /// The raw step count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A heart rate in beats per minute, within a plausible human range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HeartRate(u16);

impl HeartRate {
    /// Lowest beats-per-minute accepted (well below a trained-athlete resting
    /// rate; below this is treated as a sensor error).
    pub const MIN_BPM: u16 = 20;
    /// Highest beats-per-minute accepted (above a maximal-effort rate; beyond
    /// this is treated as a sensor error).
    pub const MAX_BPM: u16 = 250;

    /// Construct a validated heart rate.
    ///
    /// # Errors
    /// Returns [`MetricError::TooLow`] / [`MetricError::TooHigh`] outside the
    /// plausible [`HeartRate::MIN_BPM`]..=[`HeartRate::MAX_BPM`] range.
    pub const fn new(bpm: u16) -> Result<Self, MetricError> {
        if bpm < Self::MIN_BPM {
            return Err(MetricError::TooLow);
        }
        if bpm > Self::MAX_BPM {
            return Err(MetricError::TooHigh);
        }
        Ok(Self(bpm))
    }

    /// The raw beats-per-minute.
    #[must_use]
    pub const fn bpm(self) -> u16 {
        self.0
    }
}

/// A sleep duration in whole minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SleepDuration(u16);

impl SleepDuration {
    /// Most minutes of sleep accepted for one rest period (24h); beyond this is
    /// treated as a logging error.
    pub const MAX_MINUTES: u16 = 24 * 60;

    /// Construct a validated sleep duration from whole minutes.
    ///
    /// # Errors
    /// Returns [`MetricError::TooHigh`] above [`SleepDuration::MAX_MINUTES`].
    pub const fn from_minutes(minutes: u16) -> Result<Self, MetricError> {
        if minutes > Self::MAX_MINUTES {
            return Err(MetricError::TooHigh);
        }
        Ok(Self(minutes))
    }

    /// Construct from whole hours (convenience for goals expressed in hours).
    ///
    /// # Errors
    /// Returns [`MetricError::TooHigh`] above 24 hours.
    pub const fn from_hours(hours: u8) -> Result<Self, MetricError> {
        Self::from_minutes(hours as u16 * 60)
    }

    /// Total minutes of sleep.
    #[must_use]
    pub const fn minutes(self) -> u16 {
        self.0
    }
}

/// Minutes of moderate-to-vigorous activity in a period (typically one day).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActiveMinutes(u16);

impl ActiveMinutes {
    /// Most active minutes accepted for one day (24h); beyond this is a glitch.
    pub const MAX_MINUTES: u16 = 24 * 60;

    /// Construct a validated active-minutes count.
    ///
    /// # Errors
    /// Returns [`MetricError::TooHigh`] above [`ActiveMinutes::MAX_MINUTES`].
    pub const fn new(minutes: u16) -> Result<Self, MetricError> {
        if minutes > Self::MAX_MINUTES {
            return Err(MetricError::TooHigh);
        }
        Ok(Self(minutes))
    }

    /// The raw active-minute count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A body weight in kilograms, within a plausible human range.
///
/// Stored as a validated `f64`; construction rejects non-finite or
/// out-of-range values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyWeight(f64);

impl BodyWeight {
    /// Lowest kilograms accepted (below a small newborn is a logging error).
    pub const MIN_KG: f64 = 1.0;
    /// Highest kilograms accepted (beyond the heaviest recorded human).
    pub const MAX_KG: f64 = 650.0;

    /// Construct a validated body weight in kilograms.
    ///
    /// # Errors
    /// Returns [`MetricError::NotFinite`] for `NaN`/infinite input, or
    /// [`MetricError::TooLow`] / [`MetricError::TooHigh`] outside the plausible
    /// range.
    pub fn new(kg: f64) -> Result<Self, MetricError> {
        if !kg.is_finite() {
            return Err(MetricError::NotFinite);
        }
        if kg < Self::MIN_KG {
            return Err(MetricError::TooLow);
        }
        if kg > Self::MAX_KG {
            return Err(MetricError::TooHigh);
        }
        Ok(Self(kg))
    }

    /// The weight in kilograms.
    #[must_use]
    pub const fn kg(self) -> f64 {
        self.0
    }
}

/// One day's worth of aggregated metrics for a caller-supplied day tick.
///
/// `day` is an opaque ordinal the *caller* assigns (e.g. days since some epoch
/// the caller owns). This crate never reads a clock — time is always passed in,
/// keeping the engine pure and testable (Charter §7 / ADR-025 on-device).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyMetrics {
    /// Caller-assigned day ordinal. Used only for ordering and streaks.
    pub day: u32,
    /// Steps walked that day.
    pub steps: Steps,
    /// Moderate-to-vigorous active minutes that day.
    pub active: ActiveMinutes,
    /// Sleep recorded for the night belonging to that day.
    pub sleep: SleepDuration,
    /// Resting heart rate observed that day.
    pub resting_hr: HeartRate,
}

impl DailyMetrics {
    /// Assemble a day's aggregate from already-validated metrics.
    #[must_use]
    pub const fn new(
        day: u32,
        steps: Steps,
        active: ActiveMinutes,
        sleep: SleepDuration,
        resting_hr: HeartRate,
    ) -> Self {
        Self {
            day,
            steps,
            active,
            sleep,
            resting_hr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_accepts_zero_and_rejects_glitch() {
        assert_eq!(Steps::new(0).map(Steps::get), Ok(0));
        assert_eq!(Steps::new(12_000).map(Steps::get), Ok(12_000));
        assert_eq!(Steps::new(Steps::MAX).map(Steps::get), Ok(Steps::MAX));
        assert_eq!(Steps::new(Steps::MAX + 1), Err(MetricError::TooHigh));
    }

    #[test]
    fn heart_rate_range_boundaries() {
        assert_eq!(HeartRate::new(HeartRate::MIN_BPM - 1), Err(MetricError::TooLow));
        assert_eq!(HeartRate::new(HeartRate::MIN_BPM).map(HeartRate::bpm), Ok(20));
        assert_eq!(HeartRate::new(60).map(HeartRate::bpm), Ok(60));
        assert_eq!(HeartRate::new(HeartRate::MAX_BPM).map(HeartRate::bpm), Ok(250));
        assert_eq!(HeartRate::new(HeartRate::MAX_BPM + 1), Err(MetricError::TooHigh));
    }

    #[test]
    fn sleep_duration_minutes_and_hours() {
        assert_eq!(SleepDuration::from_minutes(0).map(SleepDuration::minutes), Ok(0));
        assert_eq!(SleepDuration::from_hours(8).map(SleepDuration::minutes), Ok(480));
        assert_eq!(
            SleepDuration::from_minutes(SleepDuration::MAX_MINUTES).map(SleepDuration::minutes),
            Ok(1440)
        );
        assert_eq!(
            SleepDuration::from_minutes(SleepDuration::MAX_MINUTES + 1),
            Err(MetricError::TooHigh)
        );
    }

    #[test]
    fn active_minutes_boundaries() {
        assert_eq!(ActiveMinutes::new(0).map(ActiveMinutes::get), Ok(0));
        assert_eq!(ActiveMinutes::new(30).map(ActiveMinutes::get), Ok(30));
        assert_eq!(
            ActiveMinutes::new(ActiveMinutes::MAX_MINUTES + 1),
            Err(MetricError::TooHigh)
        );
    }

    #[test]
    fn body_weight_validates_finite_and_range() {
        assert_eq!(BodyWeight::new(f64::NAN), Err(MetricError::NotFinite));
        assert_eq!(BodyWeight::new(f64::INFINITY), Err(MetricError::NotFinite));
        assert_eq!(BodyWeight::new(0.5), Err(MetricError::TooLow));
        assert_eq!(BodyWeight::new(BodyWeight::MAX_KG + 1.0), Err(MetricError::TooHigh));
        let kg = BodyWeight::new(72.5).expect("72.5 kg is in range").kg();
        assert!((kg - 72.5).abs() < 1e-9);
    }

    #[test]
    fn daily_metrics_assembles() {
        let d = DailyMetrics::new(
            10,
            Steps::new(8_000).unwrap_or(Steps(0)),
            ActiveMinutes::new(25).unwrap_or(ActiveMinutes(0)),
            SleepDuration::from_hours(8).unwrap_or(SleepDuration(0)),
            HeartRate::new(58).unwrap_or(HeartRate(60)),
        );
        assert_eq!(d.day, 10);
        assert_eq!(d.steps.get(), 8_000);
        assert_eq!(d.resting_hr.bpm(), 58);
    }
}
