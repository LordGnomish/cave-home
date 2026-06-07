// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl orchestration servicelb …` command surface — scaffold.
//!
//! ADR-005 §2: the CLI is the advanced power-user path onto the orchestration
//! layer. ServiceLB (K3s svclb / klipper-lb) gets two read verbs:
//!
//!   cavehomectl orchestration servicelb status            # cluster LB summary
//!   cavehomectl orchestration servicelb describe <svc>    # one Service's detail
//!
//! `status` renders the [`cave_home_klipper_lb_rs::controller::ServiceLbMetrics`]
//! gauges; `describe` renders one Service's svclb DaemonSet + ingress disposition.
//! Like the sibling k3s infra modules (`apiserver`/`scheduler`/`cni`) the live
//! data source (apiserver client + informer) attaches in Phase 1b; this module
//! pins the command shape the wiring fills in.

/// The leaf subcommands under `orchestration servicelb`.
#[must_use]
pub fn servicelb_subcommands() -> Vec<&'static str> {
    vec!["status", "describe"]
}

/// The command path this surface lives at: `orchestration servicelb`.
#[must_use]
pub fn command_path() -> [&'static str; 2] {
    ["orchestration", "servicelb"]
}

/// Whether the given subcommand takes a positional `<service>` argument.
/// `describe` targets one Service; `status` is cluster-wide.
#[must_use]
pub fn subcommand_takes_service(subcommand: &str) -> bool {
    subcommand == "describe"
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommands_listed() {
        let sc = servicelb_subcommands();
        assert!(sc.contains(&"status"));
        assert!(sc.contains(&"describe"));
    }

    #[test]
    fn only_describe_takes_a_service() {
        assert!(subcommand_takes_service("describe"));
        assert!(!subcommand_takes_service("status"));
    }
}
