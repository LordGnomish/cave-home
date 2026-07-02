// SPDX-License-Identifier: Apache-2.0
//! RED — failing test for the Portal **Storage** developer card (Track 3): the
//! orchestration > Storage page (PV/PVC table + hostPath). References
//! `Card::Storage`, not yet implemented.

use cave_home_portal::card::Card;

#[test]
fn storage_card_is_developer_only() {
    // The Storage page surfaces hostPath/PV internals — developer-only, hidden
    // from residents and the mobile app (Charter §6.3), like ClusterTopology.
    assert!(Card::Storage.is_developer_only());
}

#[test]
fn storage_card_is_not_a_resident_card() {
    // Sanity: a resident card is still not developer-only.
    assert!(!Card::Entity { entity_id: "x".into() }.is_developer_only());
}
