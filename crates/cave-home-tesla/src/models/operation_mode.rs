// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall operation mode.

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
