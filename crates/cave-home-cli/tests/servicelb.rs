// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl orchestration servicelb …` command-surface tests
//! (RED until the `servicelb` CLI module lands).
//!
//! The ServiceLB power-user surface (ADR-005 §2): `orchestration servicelb
//! status` (cluster-wide LB summary) and `orchestration servicelb describe
//! <service>` (one Service's svclb DaemonSet + ingress detail). Like the sibling
//! k3s infra modules (apiserver/scheduler/cni) this is the scaffolded command
//! surface; the live backend attaches with the apiserver/informer wiring.

use cave_home_cli::servicelb;

#[test]
fn servicelb_exposes_status_and_describe() {
    let sc = servicelb::servicelb_subcommands();
    assert!(sc.contains(&"status"), "missing `status` subcommand");
    assert!(sc.contains(&"describe"), "missing `describe` subcommand");
}

#[test]
fn servicelb_command_path_is_orchestration_servicelb() {
    assert_eq!(servicelb::command_path(), ["orchestration", "servicelb"]);
}

#[test]
fn describe_requires_a_service_argument() {
    // `describe` takes a positional <service>; `status` does not.
    assert!(servicelb::subcommand_takes_service("describe"));
    assert!(!servicelb::subcommand_takes_service("status"));
}
