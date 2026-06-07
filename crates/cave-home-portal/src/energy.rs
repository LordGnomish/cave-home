// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Portal `/energy` page view-model: the live power-flow diagram
//! (Solar → Home → Battery → Grid), the state-of-charge bar, the history
//! graph, the operation-mode selector and the backup-reserve toggle.
//!
//! Like the rest of `cave-home-portal` this is a **pure UI model** — std-only,
//! no network, no dependency on the device adapters. The energy backend
//! (`cave-home-tesla`) feeds it plain numbers (watts, percent, kWh); this module
//! turns them into a grandma-friendly, localised page (Charter §6.3).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::label::Lang;

    #[test]
    fn flow_sunny_charging_and_exporting() {
        // 5 kW sun, 1 kW house, charging 1 kW, exporting 3 kW.
        let flow = EnergyFlowView::from_powers(5000.0, 1000.0, -1000.0, -3000.0, 75.0);
        let edges = flow.active_edges();
        assert!(edges.iter().any(|e| e.from == FlowNode::Solar && e.to == FlowNode::Battery));
        assert!(edges.iter().any(|e| e.from == FlowNode::Solar && e.to == FlowNode::Grid));
        assert!(edges.iter().any(|e| e.from == FlowNode::Solar && e.to == FlowNode::Home));
        assert!(!edges.iter().any(|e| e.from == FlowNode::Grid));
        assert!(!edges.iter().any(|e| e.from == FlowNode::Battery));
        // Solar→Home = 5000 - 1000(charge) - 3000(export) = 1000 W.
        let to_home = edges
            .iter()
            .find(|e| e.from == FlowNode::Solar && e.to == FlowNode::Home)
            .unwrap();
        assert!((to_home.watts - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn flow_night_discharging_and_importing() {
        let flow = EnergyFlowView::from_powers(0.0, 2000.0, 1500.0, 500.0, 40.0);
        let edges = flow.active_edges();
        assert!(edges.iter().any(|e| e.from == FlowNode::Battery && e.to == FlowNode::Home));
        assert!(edges.iter().any(|e| e.from == FlowNode::Grid && e.to == FlowNode::Home));
        assert!(!edges.iter().any(|e| e.from == FlowNode::Solar));
    }

    #[test]
    fn flow_node_labels_localised() {
        assert_ne!(FlowNode::Battery.label(Lang::En), FlowNode::Battery.label(Lang::Tr));
        for n in FlowNode::ALL {
            assert!(!n.label(Lang::En).is_empty());
        }
    }

    #[test]
    fn soc_bar_fraction_and_reserve() {
        let bar = SocBar::new(72.0, 20);
        assert!((bar.fraction() - 0.72).abs() < f64::EPSILON);
        assert!((bar.reserve_fraction() - 0.20).abs() < f64::EPSILON);
        assert!(bar.above_reserve());
        let low = SocBar::new(15.0, 20);
        assert!(!low.above_reserve());
    }

    #[test]
    fn soc_bar_fraction_is_clamped() {
        assert!((SocBar::new(130.0, 20).fraction() - 1.0).abs() < f64::EPSILON);
        assert!(SocBar::new(-5.0, 20).fraction().abs() < f64::EPSILON);
    }

    #[test]
    fn history_graph_normalises_bar_heights() {
        let g = HistoryGraph::new(
            "Last 24 hours",
            vec![("08:00".into(), 1.0), ("12:00".into(), 4.0), ("16:00".into(), 2.0)],
        );
        assert!((g.max_value() - 4.0).abs() < f64::EPSILON);
        let heights = g.normalized_heights();
        assert!((heights[1] - 1.0).abs() < f64::EPSILON); // tallest
        assert!((heights[0] - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn history_graph_empty_is_safe() {
        let g = HistoryGraph::new("Empty", vec![]);
        assert!(g.max_value().abs() < f64::EPSILON);
        assert!(g.normalized_heights().is_empty());
    }

    #[test]
    fn mode_selector_marks_current() {
        let opts = mode_selector(EnergyMode::Backup, Lang::En);
        assert_eq!(opts.len(), 3);
        let backup = opts.iter().find(|o| o.key == "backup").unwrap();
        assert!(backup.selected);
        assert_eq!(opts.iter().filter(|o| o.selected).count(), 1);
    }

    #[test]
    fn backup_toggle_carries_reserve() {
        let t = BackupToggle::new(30);
        assert_eq!(t.reserve_percent, 30);
    }

    #[test]
    fn energy_page_assembles_all_widgets() {
        let page = EnergyPage::demo(Lang::En);
        assert!(!page.title.is_empty());
        assert!(!page.flow.active_edges().is_empty());
        assert_eq!(page.modes.len(), 3);
    }

    #[test]
    fn page_labels_carry_no_implementation_jargon() {
        for lang in Lang::ALL {
            let page = EnergyPage::demo(lang);
            let mut text = page.title.clone();
            for n in FlowNode::ALL {
                text.push(' ');
                text.push_str(n.label(lang));
            }
            for o in &page.modes {
                text.push(' ');
                text.push_str(o.label);
            }
            let lower = text.to_ascii_lowercase();
            for banned in ["oauth", "fleet", "instant_power", "self_consumption", "powerwall", "watts"] {
                assert!(!lower.contains(banned), "{lang:?} leaked jargon '{banned}': {text}");
            }
        }
    }
}
