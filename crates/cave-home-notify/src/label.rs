//! Grandma-friendly rendering of notifications (Charter §6.3, ADR-007).
//!
//! A [`Priority`] never reaches the household as "priority 5" or "max" — it is
//! shown as a plain word like "Important" or "FYI", localised to EN / DE / TR
//! (the Charter §6.3 languages mandatory from M1). This module is the only place
//! a notification becomes the words a household reads, and the
//! [`render`](Notification::render) helper turns a whole notification into one
//! human line.

use crate::notification::Notification;
use crate::priority::Priority;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl Priority {
    /// A short, plain-language label for this priority — what the household
    /// sees, never a number or a protocol word.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Min, Lang::En) => "Just so you know",
            (Self::Min, Lang::De) => "Nur zur Info",
            (Self::Min, Lang::Tr) => "Bilgi olsun diye",
            (Self::Low, Lang::En) => "FYI",
            (Self::Low, Lang::De) => "Zur Info",
            (Self::Low, Lang::Tr) => "Bilgilendirme",
            (Self::Default, Lang::En) => "Reminder",
            (Self::Default, Lang::De) => "Erinnerung",
            (Self::Default, Lang::Tr) => "Hatırlatma",
            (Self::High, Lang::En) => "Important",
            (Self::High, Lang::De) => "Wichtig",
            (Self::High, Lang::Tr) => "Önemli",
            (Self::Max, Lang::En) => "Urgent — please check now",
            (Self::Max, Lang::De) => "Dringend — bitte jetzt nachsehen",
            (Self::Max, Lang::Tr) => "Acil — lütfen şimdi bakın",
        }
    }
}

impl Notification {
    /// Render this notification as one grandma-friendly line:
    /// `"<priority label>: <title> — <body>"`. If the body is empty the dash
    /// and body are omitted.
    #[must_use]
    pub fn render(&self, lang: Lang) -> String {
        let label = self.priority().label(lang);
        if self.body().is_empty() {
            format!("{label}: {}", self.title())
        } else {
            format!("{label}: {} — {}", self.title(), self.body())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topic::Topic;

    fn topic(name: &str) -> Topic {
        Topic::new(name).expect("valid test topic")
    }

    #[test]
    fn every_priority_has_three_language_labels() {
        for p in Priority::ALL {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!p.label(lang).is_empty(), "{p:?} missing label");
            }
        }
    }

    #[test]
    fn labels_are_distinct_per_priority_in_english() {
        let labels: Vec<&str> = Priority::ALL.iter().map(|p| p.label(Lang::En)).collect();
        for (i, a) in labels.iter().enumerate() {
            for b in &labels[i + 1..] {
                assert_ne!(a, b, "two priorities share an English label");
            }
        }
    }

    #[test]
    fn render_includes_label_title_and_body() {
        let n = Notification::new(topic("leak"), "Water leak", "Under the kitchen sink", 0)
            .with_priority(Priority::High);
        assert_eq!(
            n.render(Lang::En),
            "Important: Water leak — Under the kitchen sink"
        );
    }

    #[test]
    fn render_omits_empty_body() {
        let n = Notification::new(topic("door"), "Front door opened", "", 0)
            .with_priority(Priority::Low);
        assert_eq!(n.render(Lang::En), "FYI: Front door opened");
    }

    #[test]
    fn render_localises() {
        let n = Notification::new(topic("leak"), "Sızıntı", "Mutfakta", 0)
            .with_priority(Priority::Max);
        assert_eq!(n.render(Lang::Tr), "Acil — lütfen şimdi bakın: Sızıntı — Mutfakta");
        assert_eq!(n.render(Lang::De), "Dringend — bitte jetzt nachsehen: Sızıntı — Mutfakta");
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the notification UI must never surface protocol, push
        // or transport terms — the household reads plain words only.
        const BANNED: &[&str] = &[
            "QoS",
            "topic",
            "retain",
            "FCM",
            "APNs",
            "token",
            "entity_id",
            "ntfy",
            "gotify",
            "Apprise",
            "webhook",
            "SMTP",
            "priority",
            "payload",
            "ARN",
            "MQTT",
        ];
        for p in Priority::ALL {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = p.label(lang);
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "priority {p:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
