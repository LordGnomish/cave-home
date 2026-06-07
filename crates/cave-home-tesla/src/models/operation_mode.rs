// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall operation mode.

use super::Lang;

/// How the Powerwall decides when to charge, hold and discharge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMode {
    /// Use stored energy to power the home, minimising grid import.
    SelfConsumption,
    /// Hold the battery full for outage backup.
    Backup,
    /// Time-based control (a.k.a. TBC) — optimise against tariff/export.
    Autonomous,
}

impl OpMode {
    /// Every mode, in a stable order.
    pub const ALL: [Self; 3] = [Self::SelfConsumption, Self::Backup, Self::Autonomous];

    /// The Tesla wire string for this mode.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::SelfConsumption => "self_consumption",
            Self::Backup => "backup",
            Self::Autonomous => "autonomous",
        }
    }

    /// Parse a mode from its Tesla wire string.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.wire() == s)
    }

    /// Parse a mode from a CLI token, accepting friendly aliases
    /// (`self-consumption`, `tbc`).
    #[must_use]
    pub fn from_cli(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "self-consumption" | "self_consumption" | "self" => Some(Self::SelfConsumption),
            "backup" => Some(Self::Backup),
            "tbc" | "autonomous" | "time-based" => Some(Self::Autonomous),
            _ => None,
        }
    }

    /// A grandma-friendly, localised label (Charter §6.3) — no wire vocabulary.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::SelfConsumption, Lang::En) => "Power my home first",
            (Self::SelfConsumption, Lang::De) => "Zuerst mein Zuhause versorgen",
            (Self::SelfConsumption, Lang::Tr) => "Önce evimi besle",
            (Self::Backup, Lang::En) => "Keep charged for outages",
            (Self::Backup, Lang::De) => "Für Stromausfälle geladen halten",
            (Self::Backup, Lang::Tr) => "Elektrik kesintisi için dolu tut",
            (Self::Autonomous, Lang::En) => "Save me the most money",
            (Self::Autonomous, Lang::De) => "Spare am meisten Geld",
            (Self::Autonomous, Lang::Tr) => "Bana en çok parayı kazandır",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Lang;
    use super::*;

    #[test]
    fn wire_roundtrip() {
        for m in [OpMode::SelfConsumption, OpMode::Backup, OpMode::Autonomous] {
            assert_eq!(OpMode::from_wire(m.wire()), Some(m));
        }
        assert_eq!(OpMode::from_wire("nonsense"), None);
    }

    #[test]
    fn wire_values_match_tesla() {
        assert_eq!(OpMode::SelfConsumption.wire(), "self_consumption");
        assert_eq!(OpMode::Backup.wire(), "backup");
        assert_eq!(OpMode::Autonomous.wire(), "autonomous");
    }

    #[test]
    fn cli_accepts_friendly_aliases() {
        assert_eq!(OpMode::from_cli("self-consumption"), Some(OpMode::SelfConsumption));
        assert_eq!(OpMode::from_cli("self_consumption"), Some(OpMode::SelfConsumption));
        assert_eq!(OpMode::from_cli("backup"), Some(OpMode::Backup));
        assert_eq!(OpMode::from_cli("tbc"), Some(OpMode::Autonomous));
        assert_eq!(OpMode::from_cli("autonomous"), Some(OpMode::Autonomous));
        assert_eq!(OpMode::from_cli("bogus"), None);
    }

    #[test]
    fn friendly_labels_localised_and_jargon_free() {
        let label = OpMode::Backup.label(Lang::En);
        assert!(!label.is_empty());
        // Different languages render differently.
        assert_ne!(OpMode::Backup.label(Lang::En), OpMode::Backup.label(Lang::Tr));
        for m in OpMode::ALL {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let l = m.label(lang).to_ascii_lowercase();
                for banned in ["self_consumption", "autonomous", "wire", "api"] {
                    assert!(!l.contains(banned), "{m:?}/{lang:?} leaked jargon: {l}");
                }
            }
        }
    }
}
