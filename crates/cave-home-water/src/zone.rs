//! The watering-zone model — the typed inputs the irrigation engine consumes.
//!
//! A zone is one independently-controlled watering circuit: a sprinkler loop in
//! the front garden, a drip line on the vegetable bed, a hose valve on the
//! terrace. Zones are vendor-neutral here: an OpenSprinkler, Rachio, B-hyve or
//! Zigbee-valve adapter (all deferred to phase-1b, see the parity manifest)
//! maps its wire format onto these types, and everything downstream — the
//! watering decision, flow monitoring, the run sequence — works off this model
//! alone.

use crate::label::Lang;

/// What a zone is doing right now.
///
/// This mirrors the Home Assistant valve / irrigation lifecycle but is named in
/// household language. The decision engine ([`crate::decision`]) consumes the
/// configured state and the live inputs to decide whether a zone should move
/// into [`ZoneState::Watering`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneState {
    /// Configured and ready, but not watering right now.
    Idle,
    /// Watering is in progress.
    Watering,
    /// Watering was started and is temporarily held (e.g. to let pressure
    /// recover, or a manual hold), to be resumed.
    Paused,
    /// Held off because rain is expected or was recent — a rain delay.
    RainDelayed,
    /// Turned off entirely; the engine will never water it.
    Disabled,
}

impl ZoneState {
    /// Whether the engine is allowed to *start* watering from this state.
    /// A disabled or rain-delayed zone is never started; a zone already
    /// watering is not re-started.
    #[must_use]
    pub const fn can_start(self) -> bool {
        matches!(self, Self::Idle | Self::Paused)
    }
}

/// How a zone's watering amount is configured.
///
/// A zone is set up to run for a fixed duration. Construction rejects a
/// non-positive runtime so the rest of the engine never has to reason about a
/// "water for zero seconds" zone.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    id: u32,
    name: String,
    runtime_seconds: u32,
    /// Optional soil-moisture threshold in percent (0..=100). When the measured
    /// soil moisture is at or above this, the zone is already wet enough and is
    /// skipped — this is the standard "smart" / sensor-gated watering rule.
    soil_moisture_threshold: Option<f64>,
    /// Optional expected flow rate in litres per minute when this zone runs.
    /// Used by [`crate::flow`] to spot a broken valve (no flow) or a burst pipe
    /// (over-flow). `None` means the zone has no flow sensor.
    expected_flow_lpm: Option<f64>,
}

/// Why a [`Zone`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneError {
    /// The configured runtime was zero — a zone must run for some time.
    ZeroRuntime,
    /// A soil-moisture threshold was outside the sensible 0..=100 percent range.
    ThresholdOutOfRange,
    /// An expected flow rate was non-finite or not positive.
    BadFlowRate,
}

impl core::fmt::Display for ZoneError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroRuntime => f.write_str("zone runtime must be greater than zero"),
            Self::ThresholdOutOfRange => {
                f.write_str("soil-moisture threshold must be between 0 and 100 percent")
            }
            Self::BadFlowRate => f.write_str("expected flow rate must be finite and positive"),
        }
    }
}

impl std::error::Error for ZoneError {}

impl Zone {
    /// Construct a validated zone.
    ///
    /// # Errors
    /// Returns [`ZoneError`] if the runtime is zero, the optional soil-moisture
    /// threshold is outside `0..=100`, or the optional expected flow rate is
    /// non-finite or non-positive.
    pub fn new(
        id: u32,
        name: impl Into<String>,
        runtime_seconds: u32,
        soil_moisture_threshold: Option<f64>,
        expected_flow_lpm: Option<f64>,
    ) -> Result<Self, ZoneError> {
        if runtime_seconds == 0 {
            return Err(ZoneError::ZeroRuntime);
        }
        if let Some(t) = soil_moisture_threshold {
            if !t.is_finite() || !(0.0..=100.0).contains(&t) {
                return Err(ZoneError::ThresholdOutOfRange);
            }
        }
        if let Some(flow) = expected_flow_lpm {
            if !flow.is_finite() || flow <= 0.0 {
                return Err(ZoneError::BadFlowRate);
            }
        }
        Ok(Self {
            id,
            name: name.into(),
            runtime_seconds,
            soil_moisture_threshold,
            expected_flow_lpm,
        })
    }

    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn runtime_seconds(&self) -> u32 {
        self.runtime_seconds
    }

    #[must_use]
    pub const fn soil_moisture_threshold(&self) -> Option<f64> {
        self.soil_moisture_threshold
    }

    #[must_use]
    pub const fn expected_flow_lpm(&self) -> Option<f64> {
        self.expected_flow_lpm
    }

    /// A plain-language label for this zone (its name plus a localised "garden"
    /// framing). Charter §6.3: never "zone 3", never "valve GPIO".
    #[must_use]
    pub fn friendly_label(&self, lang: Lang) -> String {
        let prefix = match lang {
            Lang::En => "Watering for",
            Lang::De => "Bewässerung für",
            Lang::Tr => "Sulama:",
        };
        format!("{prefix} {}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_runtime() {
        assert_eq!(
            Zone::new(1, "Front garden", 0, None, None),
            Err(ZoneError::ZeroRuntime)
        );
    }

    #[test]
    fn rejects_threshold_out_of_range() {
        assert_eq!(
            Zone::new(1, "Bed", 600, Some(120.0), None),
            Err(ZoneError::ThresholdOutOfRange)
        );
        assert_eq!(
            Zone::new(1, "Bed", 600, Some(-1.0), None),
            Err(ZoneError::ThresholdOutOfRange)
        );
        assert_eq!(
            Zone::new(1, "Bed", 600, Some(f64::NAN), None),
            Err(ZoneError::ThresholdOutOfRange)
        );
    }

    #[test]
    fn rejects_bad_flow_rate() {
        assert_eq!(
            Zone::new(1, "Bed", 600, None, Some(0.0)),
            Err(ZoneError::BadFlowRate)
        );
        assert_eq!(
            Zone::new(1, "Bed", 600, None, Some(-3.0)),
            Err(ZoneError::BadFlowRate)
        );
        assert_eq!(
            Zone::new(1, "Bed", 600, None, Some(f64::INFINITY)),
            Err(ZoneError::BadFlowRate)
        );
    }

    #[test]
    fn accepts_valid_zone_with_boundaries() {
        let z = Zone::new(7, "Back garden", 900, Some(0.0), Some(12.5))
            .expect("valid zone");
        assert_eq!(z.id(), 7);
        assert_eq!(z.name(), "Back garden");
        assert_eq!(z.runtime_seconds(), 900);
        assert_eq!(z.soil_moisture_threshold(), Some(0.0));
        assert_eq!(z.expected_flow_lpm(), Some(12.5));
        assert!(Zone::new(1, "Bed", 1, Some(100.0), None).is_ok());
    }

    #[test]
    fn state_start_eligibility() {
        assert!(ZoneState::Idle.can_start());
        assert!(ZoneState::Paused.can_start());
        assert!(!ZoneState::Watering.can_start());
        assert!(!ZoneState::RainDelayed.can_start());
        assert!(!ZoneState::Disabled.can_start());
    }

    #[test]
    fn friendly_label_has_zone_name() {
        let z = Zone::new(2, "vegetable bed", 600, None, None).expect("valid zone");
        assert!(z.friendly_label(Lang::En).contains("vegetable bed"));
        assert!(z.friendly_label(Lang::De).contains("vegetable bed"));
        assert!(z.friendly_label(Lang::Tr).contains("vegetable bed"));
    }
}
