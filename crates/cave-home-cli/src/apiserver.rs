// SPDX-License-Identifier: Apache-2.0
//! `cavehomectl <verb>` CRUD surface backed by `cave-home-apiserver-rs`
//! — Phase 2b placeholder.
//!
//! Phase 2b CLI surface (planned):
//!   cavehomectl get <kind> [name]        # GET / LIST
//!   cavehomectl describe <kind> <name>   # GET + render
//!   cavehomectl apply -f <manifest>      # CREATE / UPDATE
//!   cavehomectl delete <kind> <name>     # DELETE

#[must_use]
pub fn apiserver_subcommands() -> Vec<&'static str> {
    vec!["get", "describe", "apply", "delete"]
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommands_listed() {
        let sc = apiserver_subcommands();
        assert!(sc.contains(&"get"));
        assert!(sc.contains(&"describe"));
        assert!(sc.contains(&"apply"));
        assert!(sc.contains(&"delete"));
    }
}
