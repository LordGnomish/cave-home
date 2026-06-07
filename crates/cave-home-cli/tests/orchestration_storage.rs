// SPDX-License-Identifier: Apache-2.0
//! RED — failing test for the `cavehomectl orchestration storage` command
//! surface (Track 2). References `cave_home_cli::storage`, not yet implemented.

use cave_home_cli::storage::orchestration_storage_subcommands;

#[test]
fn storage_subcommands_listed() {
    let sc = orchestration_storage_subcommands();
    // `cavectl orchestration storage list-pvs` + `... describe <pvc>`.
    assert!(sc.contains(&"list-pvs"));
    assert!(sc.contains(&"describe"));
}
