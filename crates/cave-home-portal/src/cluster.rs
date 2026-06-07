// SPDX-License-Identifier: Apache-2.0
//! The operator-facing K3s subsystem pages mounted in the Portal.
//!
//! The Portal is the one dashboard the household *and* the operator share. The
//! resident never sees orchestration internals (Charter §6.3), so every page
//! here is a **developer-only** [`View`] — [`crate::dashboard::Dashboard::for_mode`]
//! strips them in Resident/mobile mode.
//!
//! Each page surfaces one slice of the in-process control plane the unified
//! binary runs, so an operator can drive every linked crate from one place:
//!
//! | Page       | Backing crates                                            |
//! |------------|-----------------------------------------------------------|
//! | Cluster    | apiserver / kine / scheduler / controller-manager         |
//! | Workloads  | kubelet (pods) + add-on logs                              |
//! | Networking | klipper-lb (ServiceLB) + traefik (ingress) + cni-flannel  |
//! | Storage    | local-path provisioner                                    |
//! | Security   | secrets-encryption                                        |

use crate::area::Home;
use crate::autogen::auto_dashboard;
use crate::card::Card;
use crate::dashboard::{Dashboard, View};
use crate::label::Lang;

/// Build the full operator Portal: the auto-generated household dashboard with
/// the five orchestration subsystem pages appended. This is the mount point the
/// unified binary uses to expose every linked control-plane crate in one place;
/// residents still see only the household tabs (the appended pages are
/// developer-only and dropped by [`Dashboard::for_mode`]).
#[must_use]
pub fn operator_dashboard(home: &Home, lang: Lang) -> Dashboard {
    let mut dashboard = auto_dashboard(home, lang);
    for view in subsystem_views() {
        dashboard.push_view(view);
    }
    dashboard
}

/// The five K3s subsystem pages, in display order. All are developer-only.
#[must_use]
pub fn subsystem_views() -> Vec<View> {
    vec![
        View::developer("Cluster", "server", vec![Card::ClusterTopology]),
        View::developer(
            "Workloads",
            "apps",
            vec![Card::WorkloadList, Card::Logs { entity_id: "kube-system".into() }],
        ),
        View::developer("Networking", "lan", vec![Card::ServiceLb, Card::IngressRoutes]),
        View::developer("Storage", "database", vec![Card::StorageStatus]),
        View::developer("Security", "shield", vec![Card::SecurityStatus]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::Dashboard;
    use crate::view_mode::{Surface, ViewMode};

    #[test]
    fn mounts_all_five_subsystem_pages() {
        let titles: Vec<_> = subsystem_views().iter().map(|v| v.title.clone()).collect();
        assert_eq!(titles, ["Cluster", "Workloads", "Networking", "Storage", "Security"]);
    }

    #[test]
    fn every_subsystem_page_is_developer_only() {
        for view in subsystem_views() {
            assert!(view.developer_only, "{} must be a developer page", view.title);
        }
    }

    #[test]
    fn pages_carry_their_subsystem_cards() {
        let views = subsystem_views();
        assert!(views[0].cards.contains(&Card::ClusterTopology));
        assert!(views[1].cards.contains(&Card::WorkloadList));
        assert!(views[2].cards.contains(&Card::ServiceLb));
        assert!(views[3].cards.contains(&Card::StorageStatus));
        assert!(views[4].cards.contains(&Card::SecurityStatus));
    }

    #[test]
    fn operator_dashboard_appends_pages_to_the_household_dashboard() {
        let home = Home::new();
        let base = auto_dashboard(&home, Lang::En).views.len();
        let operator = operator_dashboard(&home, Lang::En);
        assert_eq!(operator.views.len(), base + 5, "the five subsystem pages must be appended");
        assert!(operator.views.iter().any(|v| v.title == "Cluster"));
    }

    #[test]
    fn resident_mode_hides_every_subsystem_page() {
        let mut dash = Dashboard::new();
        for view in subsystem_views() {
            dash.push_view(view);
        }
        let resident = dash.for_mode(ViewMode::resident(Surface::Portal));
        assert!(!resident.has_developer_content(), "no orchestration internals may leak to residents");
        assert!(resident.views.is_empty(), "all subsystem pages are developer-only");
    }
}
