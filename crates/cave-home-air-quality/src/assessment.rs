//! Room-level assessment: turn a set of sensor readings into one verdict.
//!
//! This is the surface the rest of cave-home (automations, Portal tiles, voice
//! responses) consumes. It grades every reading — through the EPA AQI engine
//! for AQI pollutants, through the classifiers for CO₂ / VOC / radon — and
//! aggregates worst-of, EPA's standard rule: a room is only as healthy as its
//! worst pollutant.

use crate::aqi;
use crate::category::AirCategory;
use crate::classify;
use crate::reading::{Pollutant, Reading};

/// The grade for one pollutant within a room.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PollutantGrade {
    pub pollutant: Pollutant,
    pub value: f64,
    pub category: AirCategory,
    /// The numeric EPA AQI sub-index, when this pollutant has one.
    pub aqi: Option<u16>,
}

/// The overall verdict for a room.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomAssessment {
    /// Worst-of category across every graded reading.
    pub overall: AirCategory,
    /// The pollutant driving the overall category (the "dominant" pollutant).
    pub dominant: Option<Pollutant>,
    /// Per-pollutant breakdown, in input order.
    pub grades: Vec<PollutantGrade>,
}

/// Grade a single reading into a [`PollutantGrade`].
#[must_use]
pub fn grade(reading: &Reading) -> PollutantGrade {
    let p = reading.pollutant();
    let v = reading.value();
    if p.has_epa_aqi() {
        let outcome = aqi::sub_index(p, v);
        let aqi_val = outcome.as_index();
        let category = aqi_val.map_or(AirCategory::Good, AirCategory::from_aqi);
        PollutantGrade { pollutant: p, value: v, category, aqi: aqi_val }
    } else {
        // Non-AQI pollutant: classifier always returns Some for these.
        let category = classify::classify(p, v).unwrap_or(AirCategory::Good);
        PollutantGrade { pollutant: p, value: v, category, aqi: None }
    }
}

/// Assess a room from its current readings.
///
/// With no readings the room is reported as [`AirCategory::Good`] with no
/// dominant pollutant — "nothing measured, nothing wrong to report".
#[must_use]
pub fn assess(readings: &[Reading]) -> RoomAssessment {
    let grades: Vec<PollutantGrade> = readings.iter().map(grade).collect();
    let (overall, dominant) = grades.iter().fold(
        (AirCategory::Good, None),
        |(worst, dom), g| {
            if g.category > worst {
                (g.category, Some(g.pollutant))
            } else {
                (worst, dom)
            }
        },
    );
    RoomAssessment { overall, dominant, grades }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::Lang;

    fn r(p: Pollutant, v: f64) -> Reading {
        Reading::new(p, v).expect("valid test reading")
    }

    #[test]
    fn empty_room_is_good_with_no_dominant() {
        let a = assess(&[]);
        assert_eq!(a.overall, AirCategory::Good);
        assert_eq!(a.dominant, None);
        assert!(a.grades.is_empty());
    }

    #[test]
    fn clean_room_is_good() {
        let a = assess(&[
            r(Pollutant::Pm25, 5.0),
            r(Pollutant::CarbonDioxide, 600.0),
            r(Pollutant::VocIndex, 90.0),
        ]);
        assert_eq!(a.overall, AirCategory::Good);
        assert_eq!(a.dominant, None);
    }

    #[test]
    fn worst_of_picks_dominant_pollutant() {
        // CO₂ stuffy (Sensitive), everything else Good -> CO₂ dominates.
        let a = assess(&[
            r(Pollutant::Pm25, 4.0),
            r(Pollutant::CarbonDioxide, 1300.0),
            r(Pollutant::VocIndex, 80.0),
        ]);
        assert_eq!(a.overall, AirCategory::Sensitive);
        assert_eq!(a.dominant, Some(Pollutant::CarbonDioxide));
    }

    #[test]
    fn aqi_grade_carries_numeric_index() {
        let g = grade(&r(Pollutant::Pm25, 20.0));
        assert_eq!(g.aqi, Some(71));
        assert_eq!(g.category, AirCategory::Fair);
    }

    #[test]
    fn classifier_grade_has_no_numeric_aqi() {
        let g = grade(&r(Pollutant::CarbonDioxide, 1300.0));
        assert_eq!(g.aqi, None);
        assert_eq!(g.category, AirCategory::Sensitive);
    }

    #[test]
    fn assessment_drives_grandma_friendly_advice() {
        let a = assess(&[r(Pollutant::CarbonDioxide, 1800.0)]);
        assert_eq!(a.overall, AirCategory::Poor);
        // The end-user sees plain advice, never the ppm number.
        assert_eq!(a.overall.advice(Lang::En), "Open a window to let in fresh air.");
    }

    #[test]
    fn breakdown_preserves_input_order() {
        let a = assess(&[
            r(Pollutant::Pm25, 4.0),
            r(Pollutant::Ozone, 0.04),
        ]);
        assert_eq!(a.grades.len(), 2);
        assert_eq!(a.grades[0].pollutant, Pollutant::Pm25);
        assert_eq!(a.grades[1].pollutant, Pollutant::Ozone);
    }
}
