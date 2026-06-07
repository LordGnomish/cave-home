// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Portal `/energy` page view-model.
//!
//! It models the live power-flow diagram (Solar → Home → Battery → Grid), the
//! state-of-charge bar, the history graph, the operation-mode selector and the
//! backup-reserve toggle.
//!
//! Like the rest of `cave-home-portal` this is a **pure UI model** — std-only,
//! no network, no dependency on the device adapters. The energy backend
//! (`cave-home-tesla`) feeds it plain numbers (watts, percent, kWh); this module
//! turns them into a grandma-friendly, localised page (Charter §6.3).

use crate::label::Lang;

/// A node in the power-flow diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowNode {
    /// The solar panels.
    Solar,
    /// The household load.
    Home,
    /// The home battery.
    Battery,
    /// The utility grid.
    Grid,
}

impl FlowNode {
    /// Every node, in a stable order.
    pub const ALL: [Self; 4] = [Self::Solar, Self::Home, Self::Battery, Self::Grid];

    /// A grandma-friendly, localised node label.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Solar, Lang::En) => "Sun",
            (Self::Solar, Lang::De) => "Sonne",
            (Self::Solar, Lang::Tr) => "Güneş",
            (Self::Home, Lang::En) => "Home",
            (Self::Home, Lang::De) => "Zuhause",
            (Self::Home, Lang::Tr) => "Ev",
            (Self::Battery, Lang::En) => "Battery",
            (Self::Battery, Lang::De) => "Batterie",
            (Self::Battery, Lang::Tr) => "Pil",
            (Self::Grid, Lang::En) => "Grid",
            (Self::Grid, Lang::De) => "Netz",
            (Self::Grid, Lang::Tr) => "Şebeke",
        }
    }
}

/// A directed power flow between two nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowEdge {
    /// The source node.
    pub from: FlowNode,
    /// The destination node.
    pub to: FlowNode,
    /// The power along this edge, watts.
    pub watts: f64,
}

/// The live power-flow diagram.
#[derive(Debug, Clone, PartialEq)]
pub struct EnergyFlowView {
    pv_w: f64,
    load_w: f64,
    battery_w: f64,
    grid_w: f64,
    soc_percent: f64,
}

impl EnergyFlowView {
    /// Build from the raw powers (battery negative = charging, grid negative =
    /// exporting).
    #[must_use]
    pub const fn from_powers(pv_w: f64, load_w: f64, battery_w: f64, grid_w: f64, soc_percent: f64) -> Self {
        Self {
            pv_w,
            load_w,
            battery_w,
            grid_w,
            soc_percent,
        }
    }

    /// The state of charge, percent.
    #[must_use]
    pub const fn soc_percent(&self) -> f64 {
        self.soc_percent
    }

    /// The active edges of the diagram (only links carrying power).
    #[must_use]
    pub fn active_edges(&self) -> Vec<FlowEdge> {
        let mut edges = Vec::new();
        let charge = (-self.battery_w).max(0.0); // into battery
        let discharge = self.battery_w.max(0.0); // out of battery
        let export = (-self.grid_w).max(0.0); // to grid
        let import = self.grid_w.max(0.0); // from grid

        if charge > 0.0 {
            edges.push(FlowEdge { from: FlowNode::Solar, to: FlowNode::Battery, watts: charge });
        }
        if export > 0.0 {
            edges.push(FlowEdge { from: FlowNode::Solar, to: FlowNode::Grid, watts: export });
        }
        // Solar left for the home after charging + export.
        let solar_to_home = (self.pv_w - charge - export).max(0.0);
        if solar_to_home > 0.0 {
            edges.push(FlowEdge { from: FlowNode::Solar, to: FlowNode::Home, watts: solar_to_home });
        }
        if discharge > 0.0 {
            edges.push(FlowEdge { from: FlowNode::Battery, to: FlowNode::Home, watts: discharge });
        }
        if import > 0.0 {
            edges.push(FlowEdge { from: FlowNode::Grid, to: FlowNode::Home, watts: import });
        }
        edges
    }
}

/// The state-of-charge bar with its backup-reserve marker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SocBar {
    /// State of charge, percent.
    pub percent: f64,
    /// The configured backup reserve, percent.
    pub reserve_percent: u8,
}

impl SocBar {
    /// A new bar.
    #[must_use]
    pub const fn new(percent: f64, reserve_percent: u8) -> Self {
        Self { percent, reserve_percent }
    }

    /// The fill fraction, 0.0..=1.0 (clamped).
    #[must_use]
    pub fn fraction(&self) -> f64 {
        (self.percent / 100.0).clamp(0.0, 1.0)
    }

    /// The reserve marker fraction, 0.0..=1.0.
    #[must_use]
    pub fn reserve_fraction(&self) -> f64 {
        (f64::from(self.reserve_percent) / 100.0).clamp(0.0, 1.0)
    }

    /// Whether the charge is at or above the backup reserve.
    #[must_use]
    pub fn above_reserve(&self) -> bool {
        self.percent >= f64::from(self.reserve_percent)
    }
}

/// One bar in the history graph.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryBar {
    /// The x-axis label (e.g. an hour).
    pub label: String,
    /// The value, kWh.
    pub value_kwh: f64,
}

/// The 24-hour (or other range) history graph.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryGraph {
    /// The range label (e.g. "Last 24 hours").
    pub range_label: String,
    /// The bars, oldest first.
    pub bars: Vec<HistoryBar>,
}

impl HistoryGraph {
    /// A graph from `(label, value_kwh)` pairs.
    #[must_use]
    pub fn new(range_label: impl Into<String>, points: Vec<(String, f64)>) -> Self {
        Self {
            range_label: range_label.into(),
            bars: points
                .into_iter()
                .map(|(label, value_kwh)| HistoryBar { label, value_kwh })
                .collect(),
        }
    }

    /// The largest bar value (0 if empty).
    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.bars.iter().map(|b| b.value_kwh).fold(0.0, f64::max)
    }

    /// Bar heights normalised to 0.0..=1.0 against the tallest bar.
    #[must_use]
    pub fn normalized_heights(&self) -> Vec<f64> {
        let max = self.max_value();
        if max <= 0.0 {
            return self.bars.iter().map(|_| 0.0).collect();
        }
        self.bars.iter().map(|b| b.value_kwh / max).collect()
    }
}

/// The Powerwall operation mode, as the Portal models it (independent of the
/// energy adapter to keep the Portal std-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyMode {
    /// Power the home from stored energy first.
    SelfConsumption,
    /// Hold the battery full for outages.
    Backup,
    /// Optimise against tariff/export (time-based control).
    Autonomous,
}

impl EnergyMode {
    /// Every mode, in a stable order.
    pub const ALL: [Self; 3] = [Self::SelfConsumption, Self::Backup, Self::Autonomous];

    /// The stable key shared with the backend (`self_consumption` etc.).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SelfConsumption => "self_consumption",
            Self::Backup => "backup",
            Self::Autonomous => "autonomous",
        }
    }

    /// A grandma-friendly, localised label.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::SelfConsumption, Lang::En) => "Power my home first",
            (Self::SelfConsumption, Lang::De) => "Zuerst mein Zuhause versorgen",
            (Self::SelfConsumption, Lang::Tr) => "Önce evimi besle",
            (Self::Backup, Lang::En) => "Keep charged for outages",
            (Self::Backup, Lang::De) => "Für Stromausfälle geladen halten",
            (Self::Backup, Lang::Tr) => "Kesinti için dolu tut",
            (Self::Autonomous, Lang::En) => "Save me the most money",
            (Self::Autonomous, Lang::De) => "Spare am meisten Geld",
            (Self::Autonomous, Lang::Tr) => "Bana en çok parayı kazandır",
        }
    }
}

/// One option in the mode selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeOption {
    /// The mode's stable key.
    pub key: &'static str,
    /// The localised label.
    pub label: &'static str,
    /// Whether this is the currently-selected mode.
    pub selected: bool,
}

/// Build the mode selector, marking `current` as selected.
#[must_use]
pub fn mode_selector(current: EnergyMode, lang: Lang) -> Vec<ModeOption> {
    EnergyMode::ALL
        .into_iter()
        .map(|m| ModeOption {
            key: m.key(),
            label: m.label(lang),
            selected: m == current,
        })
        .collect()
}

/// The backup-reserve toggle/slider model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupToggle {
    /// The current reserve, percent.
    pub reserve_percent: u8,
}

impl BackupToggle {
    /// A toggle at `reserve_percent`.
    #[must_use]
    pub const fn new(reserve_percent: u8) -> Self {
        Self { reserve_percent }
    }
}

/// The whole `/energy` page view-model.
#[derive(Debug, Clone, PartialEq)]
pub struct EnergyPage {
    /// The localised page title.
    pub title: String,
    /// The live flow diagram.
    pub flow: EnergyFlowView,
    /// The state-of-charge bar.
    pub soc: SocBar,
    /// The history graph.
    pub history: HistoryGraph,
    /// The operation-mode selector.
    pub modes: Vec<ModeOption>,
    /// The backup-reserve toggle.
    pub backup: BackupToggle,
}

impl EnergyPage {
    /// The localised page title.
    #[must_use]
    const fn title_for(lang: Lang) -> &'static str {
        match lang {
            Lang::En => "Energy",
            Lang::De => "Energie",
            Lang::Tr => "Enerji",
        }
    }

    /// A demo page (shown until the adapter transport is wired in Phase 1b).
    #[must_use]
    pub fn demo(lang: Lang) -> Self {
        Self {
            title: Self::title_for(lang).to_string(),
            flow: EnergyFlowView::from_powers(4200.0, 1800.0, -1000.0, -1400.0, 88.0),
            soc: SocBar::new(88.0, 20),
            history: HistoryGraph::new(
                "Last 24 hours",
                vec![
                    ("06:00".into(), 0.4),
                    ("09:00".into(), 3.1),
                    ("12:00".into(), 6.8),
                    ("15:00".into(), 4.2),
                    ("18:00".into(), 1.1),
                ],
            ),
            modes: mode_selector(EnergyMode::Autonomous, lang),
            backup: BackupToggle::new(20),
        }
    }
}

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
