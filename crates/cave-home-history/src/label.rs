//! Grandma-friendly phrasing for history (Charter §6.3, ADR-007, ADR-023).
//!
//! The numbers this engine computes never reach the household raw. A chart
//! caption says *"Average today: 21°"*, *"On for 3 hours"*, *"No data for this
//! period"* — in EN / DE / TR — and never *"trapezoidal integral"*, *"p95
//! bucket"*, *"WAL segment"* or any storage/protocol term. This module turns a
//! computed value (or its absence) into that sentence.

use crate::sample::TimeUnit;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// "No data for this period." — what a chart shows when a series is empty.
#[must_use]
pub const fn no_data(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "No data for this period.",
        Lang::De => "Keine Daten für diesen Zeitraum.",
        Lang::Tr => "Bu dönem için veri yok.",
    }
}

/// "Average today: 21°" — the average for a labelled period, with an optional
/// unit suffix (already grandma-friendly, e.g. "°", "kWh"; pass "" for none).
///
/// `period` is a pre-localised word the caller chooses ("today", "this week");
/// keeping it a parameter avoids hard-coding every period × language here.
/// `value` is rounded to a whole number — households do not want decimals on a
/// glance tile.
#[must_use]
pub fn average(lang: Lang, period: &str, value: f64, unit: &str) -> String {
    let rounded = value.round() as i64;
    match lang {
        Lang::En => format!("Average {period}: {rounded}{unit}"),
        Lang::De => format!("Durchschnitt {period}: {rounded}{unit}"),
        Lang::Tr => format!("{period} ortalama: {rounded}{unit}"),
    }
}

/// "On for 3 hours" — how long something was in a (already-localised) state.
///
/// `seconds` is the dwell time; it is phrased as whole hours and minutes, the
/// resolution a household cares about. `state_phrase` is the caller's localised
/// state word ("On", "Ein", "Açık", "Home", …).
#[must_use]
pub fn time_in_state(lang: Lang, state_phrase: &str, seconds: i64) -> String {
    let duration = humanize_duration(lang, seconds);
    match lang {
        Lang::En => format!("{state_phrase} for {duration}"),
        Lang::De => format!("{state_phrase} für {duration}"),
        Lang::Tr => format!("{duration} boyunca {state_phrase}"),
    }
}

/// Turn a span of timestamps into whole hours/minutes given the series'
/// [`TimeUnit`], then phrase it. A convenience wrapper over
/// [`time_in_state`] when the duration is in raw timestamp units rather than
/// seconds.
#[must_use]
pub fn time_in_state_units(
    lang: Lang,
    state_phrase: &str,
    span: i64,
    unit: TimeUnit,
) -> String {
    let seconds = span / unit.per_second();
    time_in_state(lang, state_phrase, seconds)
}

/// Phrase a duration in seconds as a short, whole-unit human string:
/// "3 hours", "45 minutes", "2 hours 30 minutes", "less than a minute".
#[must_use]
pub fn humanize_duration(lang: Lang, seconds: i64) -> String {
    let total_minutes = seconds.max(0) / 60;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;

    if hours == 0 && minutes == 0 {
        return match lang {
            Lang::En => "less than a minute".to_string(),
            Lang::De => "weniger als eine Minute".to_string(),
            Lang::Tr => "bir dakikadan az".to_string(),
        };
    }

    let mut parts: Vec<String> = Vec::new();
    if hours > 0 {
        parts.push(hours_word(lang, hours));
    }
    if minutes > 0 {
        parts.push(minutes_word(lang, minutes));
    }
    parts.join(" ")
}

fn hours_word(lang: Lang, hours: i64) -> String {
    match lang {
        Lang::En if hours == 1 => "1 hour".to_string(),
        Lang::En => format!("{hours} hours"),
        Lang::De if hours == 1 => "1 Stunde".to_string(),
        Lang::De => format!("{hours} Stunden"),
        Lang::Tr => format!("{hours} saat"),
    }
}

fn minutes_word(lang: Lang, minutes: i64) -> String {
    match lang {
        Lang::En if minutes == 1 => "1 minute".to_string(),
        Lang::En => format!("{minutes} minutes"),
        Lang::De if minutes == 1 => "1 Minute".to_string(),
        Lang::De => format!("{minutes} Minuten"),
        Lang::Tr => format!("{minutes} dakika"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_rounds_and_formats_all_langs() {
        assert_eq!(average(Lang::En, "today", 21.4, "°"), "Average today: 21°");
        assert_eq!(
            average(Lang::De, "heute", 20.6, "°"),
            "Durchschnitt heute: 21°"
        );
        assert_eq!(average(Lang::Tr, "bugün", 21.0, "°"), "bugün ortalama: 21°");
    }

    #[test]
    fn average_handles_empty_unit() {
        assert_eq!(average(Lang::En, "this week", 5.0, ""), "Average this week: 5");
    }

    #[test]
    fn no_data_in_all_langs() {
        assert_eq!(no_data(Lang::En), "No data for this period.");
        assert_eq!(no_data(Lang::De), "Keine Daten für diesen Zeitraum.");
        assert_eq!(no_data(Lang::Tr), "Bu dönem için veri yok.");
    }

    #[test]
    fn humanize_whole_hours() {
        assert_eq!(humanize_duration(Lang::En, 3 * 3600), "3 hours");
        assert_eq!(humanize_duration(Lang::En, 3600), "1 hour");
        assert_eq!(humanize_duration(Lang::De, 2 * 3600), "2 Stunden");
        assert_eq!(humanize_duration(Lang::Tr, 5 * 3600), "5 saat");
    }

    #[test]
    fn humanize_hours_and_minutes() {
        assert_eq!(humanize_duration(Lang::En, 2 * 3600 + 30 * 60), "2 hours 30 minutes");
        assert_eq!(humanize_duration(Lang::En, 45 * 60), "45 minutes");
        assert_eq!(humanize_duration(Lang::En, 60), "1 minute");
    }

    #[test]
    fn humanize_sub_minute_and_negative() {
        assert_eq!(humanize_duration(Lang::En, 30), "less than a minute");
        assert_eq!(humanize_duration(Lang::De, 0), "weniger als eine Minute");
        assert_eq!(humanize_duration(Lang::Tr, -5), "bir dakikadan az");
    }

    #[test]
    fn time_in_state_phrasing() {
        assert_eq!(time_in_state(Lang::En, "On", 3 * 3600), "On for 3 hours");
        assert_eq!(time_in_state(Lang::De, "Ein", 3600), "Ein für 1 Stunde");
        assert_eq!(time_in_state(Lang::Tr, "Açık", 2 * 3600), "2 saat boyunca Açık");
    }

    #[test]
    fn time_in_state_units_converts_millis() {
        // 3 hours expressed in milliseconds.
        let span = 3 * 3600 * 1000;
        assert_eq!(
            time_in_state_units(Lang::En, "On", span, TimeUnit::Millis),
            "On for 3 hours"
        );
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-023: history UX never surfaces storage/query terms.
        const BANNED: &[&str] = &[
            "LTTB",
            "bucket",
            "WAL",
            "segment",
            "compaction",
            "InfluxDB",
            "Flux",
            "trapezoidal",
            "integral",
            "p95",
            "percentile",
            "stddev",
            "MQTT",
            "entity_id",
            "rollup",
            "retention",
        ];
        let langs = [Lang::En, Lang::De, Lang::Tr];
        let mut samples: Vec<String> = Vec::new();
        for lang in langs {
            samples.push(no_data(lang).to_string());
            samples.push(average(lang, "today", 21.0, "°"));
            samples.push(time_in_state(lang, "On", 3 * 3600));
            for secs in [30, 60, 3600, 2 * 3600 + 30 * 60] {
                samples.push(humanize_duration(lang, secs));
            }
        }
        for text in &samples {
            for banned in BANNED {
                assert!(
                    !text.to_lowercase().contains(&banned.to_lowercase()),
                    "history UX leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
