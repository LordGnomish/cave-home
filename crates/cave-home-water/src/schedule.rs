//! Multi-zone sequencing — building the ordered run plan for a watering cycle.
//!
//! Home water supply rarely has the pressure to run several zones at once, so —
//! exactly like OpenSprinkler and the Home Assistant irrigation controllers —
//! cave-home runs zones **sequentially**: one finishes before the next begins.
//! This module takes the per-zone watering decisions ([`crate::decision`]) and
//! produces the ordered plan plus the total time the cycle will take, so the UI
//! can say "the garden will be watered for about 25 minutes this morning".
//!
//! It is pure: it consumes decisions the caller has already computed and never
//! touches a clock or a valve.

use crate::decision::WaterDecision;
use crate::label::Lang;
use crate::zone::Zone;

/// One step in the run plan: a zone and how long it will run, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct RunStep {
    pub zone_id: u32,
    pub zone_name: String,
    pub runtime_seconds: u32,
}

/// The ordered plan for one watering cycle.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPlan {
    /// The zones that will run, in sequence. Skipped zones are not included.
    pub steps: Vec<RunStep>,
    /// Total time the whole cycle will take, in seconds (sum of the steps).
    pub total_seconds: u32,
}

impl RunPlan {
    /// Whether the cycle will water anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// A plain-language summary for the household: how many spots, how long.
    /// Charter §6.3 — minutes, not seconds; "spots", never "zones".
    #[must_use]
    pub fn summary(&self, lang: Lang) -> String {
        let minutes = self.total_seconds.div_ceil(60);
        match (self.is_empty(), lang) {
            (true, Lang::En) => "Nothing to water right now.".to_string(),
            (true, Lang::De) => "Im Moment ist nichts zu bewässern.".to_string(),
            (true, Lang::Tr) => "Şu anda sulanacak bir yer yok.".to_string(),
            (false, Lang::En) => {
                format!("Watering {} spots for about {minutes} minutes.", self.steps.len())
            }
            (false, Lang::De) => {
                format!("Bewässere {} Stellen für etwa {minutes} Minuten.", self.steps.len())
            }
            (false, Lang::Tr) => {
                format!("{} yer yaklaşık {minutes} dakika sulanıyor.", self.steps.len())
            }
        }
    }
}

/// Build the sequential run plan from zones paired with their decisions.
///
/// Input order is preserved (callers order zones by priority or by physical
/// circuit number). Only zones whose decision is "water" appear in the plan;
/// the total is the sum of their adjusted runtimes, saturating rather than
/// overflowing on an absurdly long cycle.
#[must_use]
pub fn plan_run<'a, I>(zones_with_decisions: I) -> RunPlan
where
    I: IntoIterator<Item = (&'a Zone, WaterDecision)>,
{
    let mut steps = Vec::new();
    let mut total: u32 = 0;
    for (zone, decision) in zones_with_decisions {
        if decision.water && decision.runtime_seconds > 0 {
            total = total.saturating_add(decision.runtime_seconds);
            steps.push(RunStep {
                zone_id: zone.id(),
                zone_name: zone.name().to_string(),
                runtime_seconds: decision.runtime_seconds,
            });
        }
    }
    RunPlan { steps, total_seconds: total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{decide, SkipReason};
    use crate::zone::ZoneState;

    fn zone(id: u32, name: &str, secs: u32) -> Zone {
        Zone::new(id, name, secs, Some(40.0), None).expect("valid zone")
    }

    #[test]
    fn plan_preserves_order_and_sums_duration() {
        let front = zone(1, "Front garden", 600);
        let beds = zone(2, "Vegetable beds", 900);
        let terrace = zone(3, "Terrace pots", 300);
        let dry = Some(10.0);
        let plan = plan_run([
            (&front, decide(&front, ZoneState::Idle, dry, false, 100, true)),
            (&beds, decide(&beds, ZoneState::Idle, dry, false, 100, true)),
            (&terrace, decide(&terrace, ZoneState::Idle, dry, false, 100, true)),
        ]);
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].zone_id, 1);
        assert_eq!(plan.steps[1].zone_id, 2);
        assert_eq!(plan.steps[2].zone_id, 3);
        assert_eq!(plan.total_seconds, 600 + 900 + 300);
    }

    #[test]
    fn skipped_zones_are_excluded_from_plan() {
        let front = zone(1, "Front garden", 600);
        let beds = zone(2, "Vegetable beds", 900);
        // Beds are already moist -> skipped, so only the front runs.
        let front_d = decide(&front, ZoneState::Idle, Some(10.0), false, 100, true);
        let beds_d = decide(&beds, ZoneState::Idle, Some(80.0), false, 100, true);
        assert_eq!(beds_d.reason, Some(SkipReason::SoilMoistSufficient));
        let plan = plan_run([(&front, front_d), (&beds, beds_d)]);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].zone_id, 1);
        assert_eq!(plan.total_seconds, 600);
    }

    #[test]
    fn empty_plan_when_all_skipped() {
        let front = zone(1, "Front garden", 600);
        // Rain delay holds everything off.
        let d = decide(&front, ZoneState::Idle, Some(10.0), true, 100, true);
        let plan = plan_run([(&front, d)]);
        assert!(plan.is_empty());
        assert_eq!(plan.total_seconds, 0);
    }

    #[test]
    fn seasonal_adjust_flows_into_total_duration() {
        let front = zone(1, "Front garden", 600);
        // 150 % of 10 min = 15 min = 900 s.
        let d = decide(&front, ZoneState::Idle, Some(10.0), false, 150, true);
        let plan = plan_run([(&front, d)]);
        assert_eq!(plan.total_seconds, 900);
    }

    #[test]
    fn summary_reports_minutes_and_spot_count() {
        let front = zone(1, "Front garden", 600);
        let beds = zone(2, "Vegetable beds", 900);
        let dry = Some(10.0);
        let plan = plan_run([
            (&front, decide(&front, ZoneState::Idle, dry, false, 100, true)),
            (&beds, decide(&beds, ZoneState::Idle, dry, false, 100, true)),
        ]);
        // 1500 s -> 25 minutes, 2 spots.
        let s = plan.summary(Lang::En);
        assert!(s.contains("2 spots"));
        assert!(s.contains("25 minutes"));
    }

    #[test]
    fn empty_plan_summary_is_friendly() {
        let plan = RunPlan { steps: Vec::new(), total_seconds: 0 };
        assert_eq!(plan.summary(Lang::En), "Nothing to water right now.");
    }
}
