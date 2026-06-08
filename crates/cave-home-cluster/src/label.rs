//! Grandma-friendly status text (Charter §6.3, ADR-007).
//!
//! Everything in this crate computes in cluster vocabulary — primary, backup,
//! heartbeat, fencing, quorum. None of that ever reaches the homeowner. This
//! module is the only place that turns a decision into a sentence, and it speaks
//! *home-world* language in EN / DE / TR: "Your hub is healthy", "Backup hub
//! took over — everything still works", "Updating — everything still works".
//!
//! The [`tests::ui_strings_carry_no_implementation_jargon`] test mechanically
//! enforces that no banned term (etcd, kubelet, quorum, fencing, lease, …) leaks
//! into any user-facing string.

use crate::failover::FailoverPlan;
use crate::quorum::ClusterStatus;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl ClusterStatus {
    /// One-line, plain-language status the homeowner sees on the dashboard tile.
    #[must_use]
    pub const fn headline(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Operational, Lang::En) => "Your hub is healthy.",
            (Self::Operational, Lang::De) => "Ihr Hub ist gesund.",
            (Self::Operational, Lang::Tr) => "Eviniz sağlıklı çalışıyor.",
            (Self::Degraded, Lang::En) => "Everything still works — checking on a hub.",
            (Self::Degraded, Lang::De) => "Alles funktioniert noch — ein Hub wird geprüft.",
            (Self::Degraded, Lang::Tr) => "Her şey hâlâ çalışıyor — bir hub kontrol ediliyor.",
            (Self::Down, Lang::En) => "Your home needs attention — no hub is working.",
            (Self::Down, Lang::De) => "Ihr Zuhause braucht Aufmerksamkeit — kein Hub arbeitet.",
            (Self::Down, Lang::Tr) => "Eviniz ilgi bekliyor — çalışan bir hub yok.",
        }
    }
}

impl FailoverPlan {
    /// One-line, plain-language description of the failover decision for the
    /// homeowner. Never mentions promotion / fencing / split-brain.
    #[must_use]
    pub const fn headline(&self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::NoAction, Lang::En) => "Your hub is healthy.",
            (Self::NoAction, Lang::De) => "Ihr Hub ist gesund.",
            (Self::NoAction, Lang::Tr) => "Eviniz sağlıklı çalışıyor.",
            (Self::Promote { .. }, Lang::En) => "Backup hub took over — everything still works.",
            (Self::Promote { .. }, Lang::De) => {
                "Der Reserve-Hub hat übernommen — alles funktioniert weiter."
            }
            (Self::Promote { .. }, Lang::Tr) => {
                "Yedek hub devraldı — her şey çalışmaya devam ediyor."
            }
            (Self::BlockedOnFencing { .. }, Lang::En) => {
                "Checking on your main hub before the backup takes over."
            }
            (Self::BlockedOnFencing { .. }, Lang::De) => {
                "Der Haupt-Hub wird geprüft, bevor der Reserve-Hub übernimmt."
            }
            (Self::BlockedOnFencing { .. }, Lang::Tr) => {
                "Yedek hub devralmadan önce ana hub kontrol ediliyor."
            }
            (Self::NoHealthyBackup, Lang::En) => {
                "Your main hub is offline and there is no backup hub ready."
            }
            (Self::NoHealthyBackup, Lang::De) => {
                "Ihr Haupt-Hub ist offline und kein Reserve-Hub ist bereit."
            }
            (Self::NoHealthyBackup, Lang::Tr) => {
                "Ana hub çevrimdışı ve hazır bir yedek hub yok."
            }
            (Self::Failback { .. }, Lang::En) => "Your main hub is back — everything still works.",
            (Self::Failback { .. }, Lang::De) => {
                "Ihr Haupt-Hub ist zurück — alles funktioniert weiter."
            }
            (Self::Failback { .. }, Lang::Tr) => {
                "Ana hub geri döndü — her şey çalışmaya devam ediyor."
            }
        }
    }
}

/// The plain-language line shown while a rolling update is in progress.
#[must_use]
pub const fn updating_headline(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Updating — everything still works.",
        Lang::De => "Wird aktualisiert — alles funktioniert weiter.",
        Lang::Tr => "Güncelleniyor — her şey çalışmaya devam ediyor.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUSES: [ClusterStatus; 3] = [
        ClusterStatus::Operational,
        ClusterStatus::Degraded,
        ClusterStatus::Down,
    ];
    const LANGS: [Lang; 3] = [Lang::En, Lang::De, Lang::Tr];

    fn all_plans() -> Vec<FailoverPlan> {
        vec![
            FailoverPlan::NoAction,
            FailoverPlan::Promote { node: "hub-2".to_owned(), demote: Some("hub-1".to_owned()) },
            FailoverPlan::BlockedOnFencing { candidate: "hub-2".to_owned() },
            FailoverPlan::NoHealthyBackup,
            FailoverPlan::Failback { to: "hub-1".to_owned(), demote: Some("hub-2".to_owned()) },
        ]
    }

    #[test]
    fn every_status_has_three_language_headlines() {
        for s in STATUSES {
            for l in LANGS {
                assert!(!s.headline(l).is_empty());
            }
        }
    }

    #[test]
    fn every_failover_plan_has_three_language_headlines() {
        for p in all_plans() {
            for l in LANGS {
                assert!(!p.headline(l).is_empty());
            }
        }
    }

    #[test]
    fn updating_headline_localised() {
        assert_eq!(updating_headline(Lang::En), "Updating — everything still works.");
        assert!(!updating_headline(Lang::De).is_empty());
        assert!(!updating_headline(Lang::Tr).is_empty());
    }

    #[test]
    fn promotion_uses_grandma_words() {
        // The headline a homeowner reads when the backup takes over.
        assert_eq!(
            FailoverPlan::Promote { node: "x".to_owned(), demote: None }.headline(Lang::En),
            "Backup hub took over — everything still works."
        );
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-004 / ADR-005: the UI must never surface cluster /
        // orchestration internals. No homeowner should ever read these words.
        const BANNED: &[&str] = &[
            "etcd", "kubelet", "kube", "pod", "namespace", "RBAC", "quorum",
            "fencing", "fence", "STONITH", "split-brain", "lease", "Raft",
            "promote", "promotion", "demote", "heartbeat", "gossip", "node",
            "kine", "K3s", "apiserver", "cordon", "drain", "PAN-ID",
            "MQTT topic", "Helm chart",
        ];
        // Gather every user-facing string this module can emit.
        let mut texts: Vec<String> = Vec::new();
        for s in STATUSES {
            for l in LANGS {
                texts.push(s.headline(l).to_owned());
            }
        }
        for p in all_plans() {
            for l in LANGS {
                texts.push(p.headline(l).to_owned());
            }
        }
        for l in LANGS {
            texts.push(updating_headline(l).to_owned());
        }
        for text in texts {
            let lower = text.to_lowercase();
            for banned in BANNED {
                assert!(
                    !lower.contains(&banned.to_lowercase()),
                    "UI string leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
