//! `cave-home-water` — irrigation intelligence for cave-home (ADR-013).
//!
//! This crate is the **brain** that turns garden conditions into a watering
//! verdict a household can act on: for each watering circuit it decides whether
//! to water now and for how long (applying a seasonal/weather adjustment,
//! gating on soil moisture, honouring rain delays), explains *why* when it
//! skips, watches the flow while a circuit runs to spot a stuck valve or a
//! burst pipe, and lays out the whole cycle as a single sequential run plan —
//! all surfaced in plain language in EN / DE / TR.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`zone`] — the vendor-neutral watering-circuit model.
//! - [`decision`] — the watering decision engine: seasonal-adjust math,
//!   soil-moisture gating, rain-delay / disabled / window handling, and a
//!   [`decision::SkipReason`] the UI can explain.
//! - [`flow`] — flow monitoring: no-flow (stuck valve / cut supply) and
//!   over-flow (burst pipe / leak) detection against a tolerance band.
//! - [`schedule`] — sequential multi-zone run planning + total cycle duration.
//! - [`label`] — the localisation surface (Charter §6.3, ADR-007).
//!
//! The **vendor I/O adapters** (OpenSprinkler, Rachio, B-hyve, Zigbee/Z-Wave
//! valves), the **live weather / evapotranspiration feeds**, the
//! **cave-home-core entity/state integration** and **real timezone-aware
//! scheduling triggers** are network / hardware / clock-bound and are deferred
//! to Phase 1b — every one is enumerated in `parity.manifest.toml`
//! `[[unmapped]]` with an ADR-013 disposition. They feed their inputs into this
//! engine (a [`zone::Zone`], a soil-moisture reading, a rain-delay flag, a
//! seasonal percentage, a window flag) and reuse it unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_water::{decide, plan_run, Zone, ZoneState, Lang};
//!
//! // A 10-minute front-garden circuit that skips when soil is above 40% moist.
//! let front = Zone::new(1, "Front garden", 600, Some(40.0), None).unwrap();
//!
//! // Dry soil, no rain, full-season runtime, inside the morning window.
//! let decision = decide(&front, ZoneState::Idle, Some(20.0), false, 100, true);
//! assert!(decision.water);
//! assert_eq!(decision.runtime_seconds, 600);
//!
//! // The household sees a plain-language plan, never a runtime in seconds.
//! let plan = plan_run([(&front, decision)]);
//! println!("{}", plan.summary(Lang::En));
//! ```

pub mod decision;
pub mod flow;
pub mod label;
pub mod schedule;
pub mod zone;

pub use decision::{apply_seasonal_adjust, decide, SkipReason, WaterDecision};
pub use flow::{detect, FlowFault};
pub use label::Lang;
pub use schedule::{plan_run, RunPlan, RunStep};
pub use zone::{Zone, ZoneError, ZoneState};

#[cfg(test)]
mod tests {
    use super::*;

    /// Charter §6.3: no implementation jargon may leak into any user-facing
    /// string this crate produces. We exercise every localised surface in all
    /// three languages and assert none contains a banned term. Mirrors the
    /// air-quality crate's `ui_strings_carry_no_implementation_jargon`.
    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        const BANNED: &[&str] = &[
            "valve", "GPIO", "MQTT", "entity_id", "Zigbee", "Z-Wave", "Modbus",
            "M-Bus", "OpenSprinkler", "Rachio", "lpm", "seconds", "threshold",
            "flow rate", "API", "pod", "kubelet",
        ];
        let langs = [Lang::En, Lang::De, Lang::Tr];

        let zone = Zone::new(1, "the back garden", 600, Some(40.0), None)
            .expect("valid zone");

        let mut strings: Vec<String> = Vec::new();

        for lang in langs {
            // Zone label.
            strings.push(zone.friendly_label(lang));
            // Every skip reason's explanation.
            for reason in [
                SkipReason::SoilMoistSufficient,
                SkipReason::RainDelay,
                SkipReason::ZoneDisabled,
                SkipReason::OutsideWindow,
                SkipReason::SeasonalZero,
            ] {
                strings.push(reason.explain(lang).to_string());
            }
            // Decision explanations (watering + a skip).
            let watering = decide(&zone, ZoneState::Idle, Some(10.0), false, 100, true);
            let skipped = decide(&zone, ZoneState::Idle, Some(90.0), false, 100, true);
            strings.push(watering.explain(&zone, lang));
            strings.push(skipped.explain(&zone, lang));
            // Every flow-fault alert.
            for fault in [FlowFault::Healthy, FlowFault::NoFlow, FlowFault::OverFlow] {
                strings.push(fault.alert("the back garden", lang));
            }
            // Run-plan summaries (with steps and empty).
            let plan = plan_run([(&zone, watering)]);
            strings.push(plan.summary(lang));
            strings.push(RunPlan { steps: Vec::new(), total_seconds: 0 }.summary(lang));
        }

        for text in &strings {
            for banned in BANNED {
                assert!(
                    !text.to_lowercase().contains(&banned.to_lowercase()),
                    "user-facing string leaks jargon {banned:?}: {text:?}"
                );
            }
        }
    }

    #[test]
    fn engine_produces_a_complete_morning_plan() {
        // A small garden: front lawn (dry), beds (already moist), terrace (dry).
        let lawn = Zone::new(1, "Front lawn", 600, Some(40.0), Some(10.0))
            .expect("valid zone");
        let beds = Zone::new(2, "Vegetable beds", 900, Some(40.0), None)
            .expect("valid zone");
        let terrace = Zone::new(3, "Terrace pots", 300, None, None)
            .expect("valid zone");

        let lawn_d = decide(&lawn, ZoneState::Idle, Some(15.0), false, 120, true);
        let beds_d = decide(&beds, ZoneState::Idle, Some(70.0), false, 120, true);
        let terrace_d = decide(&terrace, ZoneState::Idle, None, false, 120, true);

        // Beds skip (moist); lawn + terrace water at 120% season.
        assert_eq!(beds_d.reason, Some(SkipReason::SoilMoistSufficient));
        let plan = plan_run([(&lawn, lawn_d), (&beds, beds_d), (&terrace, terrace_d)]);
        assert_eq!(plan.steps.len(), 2);
        // 120% of 600 = 720, 120% of 300 = 360 -> 1080 s.
        assert_eq!(plan.total_seconds, 1080);

        // Flow check on the lawn: 10 lpm expected, 0 measured -> stuck valve.
        let fault = detect(10.0, 0.0, 0.2).expect("judgement possible");
        assert_eq!(fault, FlowFault::NoFlow);
        assert!(fault.is_fault());
    }
}
