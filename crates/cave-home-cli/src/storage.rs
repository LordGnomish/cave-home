// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl orchestration storage ...` sub-commands — Phase 2b placeholder.
//!
//! The local-path-provisioner decision core lives in
//! `cave-home-orchestration::local_path_provisioner`; its
//! `report::StorageReport` is the view-model these commands render. As with the
//! sibling apiserver/scheduler/kubelet placeholders, the command names are
//! declared here and the backend is attached in Phase 2b (ADR-004 phase-1b).
//!
//! Phase 2b CLI surface (planned):
//!   cavehomectl orchestration storage list-pvs        # PV/PVC table + hostPath
//!   cavehomectl orchestration storage describe <pvc>  # one volume's detail

#[must_use]
pub fn orchestration_storage_subcommands() -> Vec<&'static str> {
    vec!["list-pvs", "describe"]
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommands_listed() {
        let sc = orchestration_storage_subcommands();
        assert!(sc.contains(&"list-pvs"));
        assert!(sc.contains(&"describe"));
    }
}
