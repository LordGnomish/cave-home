//! Flow monitoring — spotting a broken valve or a burst pipe while watering.
//!
//! When a zone runs, cave-home compares the **measured** flow rate against the
//! zone's **expected** flow rate ([`crate::zone::Zone::expected_flow_lpm`]).
//! Two faults matter to a household:
//!
//! - **No flow** — the valve opened but (almost) nothing is flowing. The valve
//!   is stuck shut, the supply is off, or a fitting popped off upstream. The
//!   garden silently goes dry.
//! - **Over-flow** — far more water is flowing than the zone should draw. A
//!   pipe has burst or a line is leaking; left running, this is the surprise
//!   water bill (and possibly a flood) the Charter §2 persona dreads.
//!
//! Both are judged against a symmetric tolerance band around the expected
//! rate, so normal pressure variation does not raise a false alarm. This is
//! first-party logic (ADR-013: leak-detection is first-party Rust); it takes
//! the readings the caller has already gathered and performs no I/O.

use crate::label::Lang;

/// A detected flow fault, or the all-clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowFault {
    /// Flow is within tolerance of what was expected — nothing wrong.
    Healthy,
    /// Little or no water is flowing — likely a stuck valve or a cut supply.
    NoFlow,
    /// Far too much water is flowing — likely a burst pipe or a leak.
    OverFlow,
}

impl FlowFault {
    /// Whether this represents a problem worth alerting the household about.
    #[must_use]
    pub const fn is_fault(self) -> bool {
        !matches!(self, Self::Healthy)
    }

    /// A plain-language alert line for the household (Charter §6.3 — "leak",
    /// "water", never "flow-rate delta" or "valve GPIO"). `garden` is the
    /// affected zone's friendly name, woven into the sentence.
    #[must_use]
    pub fn alert(self, garden: &str, lang: Lang) -> String {
        match (self, lang) {
            (Self::Healthy, Lang::En) => format!("{garden} is watering normally."),
            (Self::Healthy, Lang::De) => format!("{garden} wird normal bewässert."),
            (Self::Healthy, Lang::Tr) => format!("{garden} normal sulanıyor."),
            (Self::NoFlow, Lang::En) => {
                format!("No water is reaching {garden} — please check the tap and the line.")
            }
            (Self::NoFlow, Lang::De) => {
                format!("Bei {garden} kommt kein Wasser an — bitte Hahn und Leitung prüfen.")
            }
            (Self::NoFlow, Lang::Tr) => {
                format!("{garden} bölgesine su ulaşmıyor — lütfen vanayı ve hattı kontrol edin.")
            }
            (Self::OverFlow, Lang::En) => {
                format!("Possible leak near {garden} — far too much water is flowing.")
            }
            (Self::OverFlow, Lang::De) => {
                format!("Möglicher Wasserschaden bei {garden} — viel zu viel Wasser fließt.")
            }
            (Self::OverFlow, Lang::Tr) => {
                format!("{garden} yakınında olası su kaçağı — çok fazla su akıyor.")
            }
        }
    }
}

/// Detect a flow fault by comparing measured against expected flow.
///
/// `tolerance` is a fraction (e.g. `0.20` for ±20 %). Measured flow within
/// `expected * (1 ± tolerance)` is [`FlowFault::Healthy`]; below the band is
/// [`FlowFault::NoFlow`]; above it is [`FlowFault::OverFlow`].
///
/// Returns `None` when the inputs cannot support a judgement: a non-finite or
/// negative measurement, a non-positive expected rate, or a negative tolerance.
/// A `None` is "can't tell", which the caller should treat as *not* an alarm.
#[must_use]
pub fn detect(expected_lpm: f64, measured_lpm: f64, tolerance: f64) -> Option<FlowFault> {
    if !expected_lpm.is_finite() || expected_lpm <= 0.0 {
        return None;
    }
    if !measured_lpm.is_finite() || measured_lpm < 0.0 {
        return None;
    }
    if !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let low = expected_lpm * (1.0 - tolerance);
    let high = expected_lpm * (1.0 + tolerance);
    if measured_lpm < low {
        Some(FlowFault::NoFlow)
    } else if measured_lpm > high {
        Some(FlowFault::OverFlow)
    } else {
        Some(FlowFault::Healthy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_tolerance_is_healthy() {
        // Expected 10 lpm, ±20 % -> [8, 12]. Measured 11 -> healthy.
        assert_eq!(detect(10.0, 11.0, 0.20), Some(FlowFault::Healthy));
        assert_eq!(detect(10.0, 10.0, 0.20), Some(FlowFault::Healthy));
    }

    #[test]
    fn below_band_is_no_flow() {
        // Stuck valve: 0 lpm measured.
        assert_eq!(detect(10.0, 0.0, 0.20), Some(FlowFault::NoFlow));
        // Just under the lower edge (8.0): 7.9.
        assert_eq!(detect(10.0, 7.9, 0.20), Some(FlowFault::NoFlow));
    }

    #[test]
    fn above_band_is_over_flow() {
        // Burst pipe: double the expected flow.
        assert_eq!(detect(10.0, 20.0, 0.20), Some(FlowFault::OverFlow));
        // Just over the upper edge (12.0): 12.1.
        assert_eq!(detect(10.0, 12.1, 0.20), Some(FlowFault::OverFlow));
    }

    #[test]
    fn band_edges_are_inclusive_healthy() {
        // Exactly at the edges is within tolerance.
        assert_eq!(detect(10.0, 8.0, 0.20), Some(FlowFault::Healthy));
        assert_eq!(detect(10.0, 12.0, 0.20), Some(FlowFault::Healthy));
    }

    #[test]
    fn rejects_unusable_inputs() {
        assert_eq!(detect(0.0, 5.0, 0.20), None);
        assert_eq!(detect(-1.0, 5.0, 0.20), None);
        assert_eq!(detect(10.0, f64::NAN, 0.20), None);
        assert_eq!(detect(10.0, -1.0, 0.20), None);
        assert_eq!(detect(10.0, 5.0, -0.1), None);
        assert_eq!(detect(f64::INFINITY, 5.0, 0.20), None);
    }

    #[test]
    fn is_fault_flags_problems_only() {
        assert!(!FlowFault::Healthy.is_fault());
        assert!(FlowFault::NoFlow.is_fault());
        assert!(FlowFault::OverFlow.is_fault());
    }

    #[test]
    fn alert_names_the_affected_garden() {
        let a = FlowFault::OverFlow.alert("the back garden", Lang::En);
        assert!(a.contains("the back garden"));
        let n = FlowFault::NoFlow.alert("the terrace", Lang::De);
        assert!(n.contains("the terrace"));
    }
}
