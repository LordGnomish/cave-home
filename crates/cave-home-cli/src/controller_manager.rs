// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl controllers ...` sub-commands — Phase 2b placeholder.
//!
//! Phase 2b CLI surface (planned):
//!   cavehomectl controllers-status        # queue depth + reconcile rates
//!   cavehomectl describe-deployment <n>   # ReplicaSet rollout fan-out
//!   cavehomectl describe-job <n>          # completions + parallelism

#[must_use]
pub fn controller_manager_subcommands() -> Vec<&'static str> {
    vec!["controllers-status", "describe-deployment", "describe-job"]
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommands_listed() {
        let sc = controller_manager_subcommands();
        assert!(sc.contains(&"controllers-status"));
        assert!(sc.contains(&"describe-deployment"));
        assert!(sc.contains(&"describe-job"));
    }
}
