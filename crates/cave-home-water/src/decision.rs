//! The watering decision engine — the brain of cave-home-water.
//!
//! Given a zone, the live conditions (current soil moisture, a rain-delay flag,
//! a seasonal-adjust percentage) and whether the moment falls inside the zone's
//! watering window, this decides **whether to water now and for how long**, and
//! — when it decides *not* to — exactly *why*, so the UI can explain itself in
//! household language ("Skipped watering — soil is still moist").
//!
//! The semantics mirror Home Assistant's irrigation/valve behaviour and the
//! OpenSprinkler-class "weather adjustment + sensor gating" rules, implemented
//! as first-party logic:
//!
//! - **Seasonal adjust** scales the configured runtime. 100 % means "run as
//!   configured"; 150 % runs half again as long in high summer; 0 % means "do
//!   not water at all this period" (winterised), which is itself a skip reason.
//! - **Soil-moisture gating**: if the bed is already at or above its moisture
//!   threshold, watering is skipped.
//! - **Rain delay**: if rain is expected or recent, watering is skipped.
//! - **Disabled / outside window**: a turned-off zone, or a moment outside the
//!   zone's allowed window, is skipped.
//!
//! The engine is a pure function of its inputs — the caller supplies the clock
//! (via `within_window`) and the weather (via `rain_delay`). It performs no I/O.

use crate::label::Lang;
use crate::zone::{Zone, ZoneState};

/// Why the engine decided **not** to water.
///
/// Each variant carries a localised, jargon-free explanation for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The soil is already at or above the zone's moisture threshold.
    SoilMoistSufficient,
    /// Rain is expected or was recent — a rain delay is in effect.
    RainDelay,
    /// The zone is turned off.
    ZoneDisabled,
    /// The moment is outside the zone's allowed watering window.
    OutsideWindow,
    /// The seasonal adjustment is 0 % — watering is suspended this period.
    SeasonalZero,
}

impl SkipReason {
    /// A plain-language explanation for the household (Charter §6.3 — no
    /// runtimes, no thresholds, no "zone" or "valve").
    #[must_use]
    pub const fn explain(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::SoilMoistSufficient, Lang::En) => "Skipped watering — soil is still moist.",
            (Self::SoilMoistSufficient, Lang::De) => {
                "Bewässerung übersprungen — der Boden ist noch feucht."
            }
            (Self::SoilMoistSufficient, Lang::Tr) => {
                "Sulama atlandı — toprak hâlâ nemli."
            }
            (Self::RainDelay, Lang::En) => "Skipped watering — rain is on the way.",
            (Self::RainDelay, Lang::De) => "Bewässerung übersprungen — es regnet bald.",
            (Self::RainDelay, Lang::Tr) => "Sulama atlandı — yağmur geliyor.",
            (Self::ZoneDisabled, Lang::En) => "Watering is turned off here.",
            (Self::ZoneDisabled, Lang::De) => "Die Bewässerung ist hier ausgeschaltet.",
            (Self::ZoneDisabled, Lang::Tr) => "Burada sulama kapalı.",
            (Self::OutsideWindow, Lang::En) => "Not watering now — it is outside the watering time.",
            (Self::OutsideWindow, Lang::De) => {
                "Jetzt keine Bewässerung — außerhalb der Bewässerungszeit."
            }
            (Self::OutsideWindow, Lang::Tr) => "Şimdi sulama yok — sulama saati dışında.",
            (Self::SeasonalZero, Lang::En) => "Watering is paused for the season.",
            (Self::SeasonalZero, Lang::De) => "Die Bewässerung pausiert für diese Jahreszeit.",
            (Self::SeasonalZero, Lang::Tr) => "Sulama bu mevsim için duraklatıldı.",
        }
    }
}

/// The result of the watering decision for one zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterDecision {
    /// Whether the zone should water now.
    pub water: bool,
    /// How long to water, in seconds, after the seasonal adjustment. Zero when
    /// the decision is to skip.
    pub runtime_seconds: u32,
    /// `None` when watering; the explanation when skipping.
    pub reason: Option<SkipReason>,
}

impl WaterDecision {
    /// A decision to water for `runtime_seconds`.
    #[must_use]
    const fn water_for(runtime_seconds: u32) -> Self {
        Self { water: true, runtime_seconds, reason: None }
    }

    /// A decision to skip, carrying the reason.
    #[must_use]
    const fn skip(reason: SkipReason) -> Self {
        Self { water: false, runtime_seconds: 0, reason: Some(reason) }
    }

    /// A plain-language line for the UI describing this decision.
    #[must_use]
    pub fn explain(&self, zone: &Zone, lang: Lang) -> String {
        match self.reason {
            Some(reason) => reason.explain(lang).to_string(),
            None => {
                let watered = match lang {
                    Lang::En => "Watering",
                    Lang::De => "Bewässere",
                    Lang::Tr => "Sulanıyor:",
                };
                format!("{watered} {}", zone.name())
            }
        }
    }
}

/// Apply a seasonal-adjust percentage to a base runtime.
///
/// 100 % is identity; 0 % yields 0; 200 % doubles. Rounded to the nearest
/// second. Saturates instead of overflowing for absurd percentages.
#[must_use]
pub fn apply_seasonal_adjust(base_seconds: u32, seasonal_percent: u32) -> u32 {
    let scaled = u64::from(base_seconds) * u64::from(seasonal_percent);
    // Round-to-nearest on the divide-by-100.
    let rounded = (scaled + 50) / 100;
    u32::try_from(rounded).unwrap_or(u32::MAX)
}

/// Decide whether and how long a zone should water.
///
/// Inputs:
/// - `zone` — the configured circuit ([`Zone`]).
/// - `state` — what the zone is currently doing ([`ZoneState`]).
/// - `current_soil_moisture` — measured soil moisture in percent, if a sensor
///   is present. `None` means "no sensor", so the moisture gate cannot fire.
/// - `rain_delayed` — `true` if rain is expected/recent (the caller supplies
///   this from its weather source).
/// - `seasonal_percent` — the weather/seasonal scaling for the runtime.
/// - `within_window` — `true` if the present moment is inside the zone's
///   allowed watering window (the caller supplies this from its clock).
///
/// The skip checks are evaluated in priority order: a disabled zone is reported
/// as disabled even if it is also rain-delayed, etc. — the UI gets the single
/// most-actionable reason.
#[must_use]
pub fn decide(
    zone: &Zone,
    state: ZoneState,
    current_soil_moisture: Option<f64>,
    rain_delayed: bool,
    seasonal_percent: u32,
    within_window: bool,
) -> WaterDecision {
    // 1. A turned-off zone never waters.
    if state == ZoneState::Disabled {
        return WaterDecision::skip(SkipReason::ZoneDisabled);
    }
    // 2. Outside its window (or otherwise not eligible to start), it waits.
    if !within_window || !state.can_start() {
        return WaterDecision::skip(SkipReason::OutsideWindow);
    }
    // 3. Rain delay holds the whole schedule off.
    if rain_delayed {
        return WaterDecision::skip(SkipReason::RainDelay);
    }
    // 4. Already wet enough? Skip (sensor-gated watering).
    if let (Some(threshold), Some(moisture)) =
        (zone.soil_moisture_threshold(), current_soil_moisture)
    {
        if moisture >= threshold {
            return WaterDecision::skip(SkipReason::SoilMoistSufficient);
        }
    }
    // 5. Seasonal adjust to 0 means "suspended this period".
    let runtime = apply_seasonal_adjust(zone.runtime_seconds(), seasonal_percent);
    if runtime == 0 {
        return WaterDecision::skip(SkipReason::SeasonalZero);
    }
    WaterDecision::water_for(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> Zone {
        // 10-minute base runtime, water below 40 % soil moisture.
        Zone::new(1, "Front garden", 600, Some(40.0), None).expect("valid zone")
    }

    #[test]
    fn waters_when_all_conditions_allow() {
        let d = decide(&zone(), ZoneState::Idle, Some(20.0), false, 100, true);
        assert!(d.water);
        assert_eq!(d.runtime_seconds, 600);
        assert_eq!(d.reason, None);
    }

    #[test]
    fn skips_when_disabled() {
        let d = decide(&zone(), ZoneState::Disabled, Some(10.0), false, 100, true);
        assert!(!d.water);
        assert_eq!(d.reason, Some(SkipReason::ZoneDisabled));
        assert_eq!(d.runtime_seconds, 0);
    }

    #[test]
    fn skips_when_outside_window() {
        let d = decide(&zone(), ZoneState::Idle, Some(10.0), false, 100, false);
        assert!(!d.water);
        assert_eq!(d.reason, Some(SkipReason::OutsideWindow));
    }

    #[test]
    fn skips_when_state_cannot_start() {
        // Already watering: not re-started.
        let d = decide(&zone(), ZoneState::Watering, Some(10.0), false, 100, true);
        assert_eq!(d.reason, Some(SkipReason::OutsideWindow));
    }

    #[test]
    fn skips_when_rain_delayed() {
        let d = decide(&zone(), ZoneState::Idle, Some(10.0), true, 100, true);
        assert!(!d.water);
        assert_eq!(d.reason, Some(SkipReason::RainDelay));
    }

    #[test]
    fn skips_when_soil_moist_sufficient() {
        // 55 % moisture, threshold 40 % -> already wet enough.
        let d = decide(&zone(), ZoneState::Idle, Some(55.0), false, 100, true);
        assert!(!d.water);
        assert_eq!(d.reason, Some(SkipReason::SoilMoistSufficient));
    }

    #[test]
    fn soil_threshold_boundary_is_inclusive_skip() {
        // Exactly at threshold counts as "wet enough" -> skip.
        let at = decide(&zone(), ZoneState::Idle, Some(40.0), false, 100, true);
        assert_eq!(at.reason, Some(SkipReason::SoilMoistSufficient));
        // Just below threshold -> water.
        let below = decide(&zone(), ZoneState::Idle, Some(39.999), false, 100, true);
        assert!(below.water);
    }

    #[test]
    fn no_sensor_means_moisture_gate_never_fires() {
        let z = Zone::new(1, "Bed", 600, Some(40.0), None).expect("valid zone");
        // No moisture reading -> cannot skip on moisture, so it waters.
        let d = decide(&z, ZoneState::Idle, None, false, 100, true);
        assert!(d.water);
    }

    #[test]
    fn zone_without_threshold_ignores_moisture() {
        let z = Zone::new(1, "Bed", 600, None, None).expect("valid zone");
        // Soaking wet, but no configured threshold -> still waters.
        let d = decide(&z, ZoneState::Idle, Some(99.0), false, 100, true);
        assert!(d.water);
    }

    #[test]
    fn skips_when_seasonal_zero() {
        let d = decide(&zone(), ZoneState::Idle, Some(10.0), false, 0, true);
        assert!(!d.water);
        assert_eq!(d.reason, Some(SkipReason::SeasonalZero));
    }

    #[test]
    fn seasonal_adjust_scales_runtime() {
        // 200 % of 10 min = 20 min.
        let d = decide(&zone(), ZoneState::Idle, Some(10.0), false, 200, true);
        assert!(d.water);
        assert_eq!(d.runtime_seconds, 1200);
        // 50 % of 10 min = 5 min.
        let half = decide(&zone(), ZoneState::Idle, Some(10.0), false, 50, true);
        assert_eq!(half.runtime_seconds, 300);
    }

    #[test]
    fn seasonal_adjust_math() {
        assert_eq!(apply_seasonal_adjust(600, 100), 600);
        assert_eq!(apply_seasonal_adjust(600, 0), 0);
        assert_eq!(apply_seasonal_adjust(600, 200), 1200);
        assert_eq!(apply_seasonal_adjust(600, 50), 300);
        // Rounds to nearest second: 33 % of 100 s = 33 s.
        assert_eq!(apply_seasonal_adjust(100, 33), 33);
        // 100 * 0.335 -> rounds to 34 (round-half-up at .5).
        assert_eq!(apply_seasonal_adjust(1000, 335), 3350);
    }

    #[test]
    fn seasonal_adjust_saturates_not_overflows() {
        assert_eq!(apply_seasonal_adjust(u32::MAX, u32::MAX), u32::MAX);
    }

    #[test]
    fn disabled_beats_rain_delay_in_priority() {
        // Both disabled and rain-delayed -> reports the more fundamental cause.
        let d = decide(&zone(), ZoneState::Disabled, Some(10.0), true, 100, true);
        assert_eq!(d.reason, Some(SkipReason::ZoneDisabled));
    }

    #[test]
    fn decision_explains_itself_in_household_language() {
        let z = zone();
        let watering = decide(&z, ZoneState::Idle, Some(10.0), false, 100, true);
        assert!(watering.explain(&z, Lang::En).contains("Front garden"));
        let skipped = decide(&z, ZoneState::Idle, Some(90.0), false, 100, true);
        assert_eq!(
            skipped.explain(&z, Lang::En),
            "Skipped watering — soil is still moist."
        );
    }
}
