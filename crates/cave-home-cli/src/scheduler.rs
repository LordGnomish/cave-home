// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl scheduler ...` sub-commands — Phase 2b placeholder.
//!
//! Phase 2b CLI surface (planned):
//!   cavehomectl describe-scheduler   # show queue depth + plugin config
//!   cavehomectl show-decisions       # tail recent ScheduleResult rows

#[must_use]
pub fn scheduler_subcommands() -> Vec<&'static str> {
    vec!["describe-scheduler", "show-decisions"]
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommands_listed() {
        let sc = scheduler_subcommands();
        assert!(sc.contains(&"describe-scheduler"));
        assert!(sc.contains(&"show-decisions"));
    }
}
