// SPDX-License-Identifier: Apache-2.0
//! Portal ServiceLB card tests (RED until the `Card::ServiceLb` variant lands).
//!
//! The Portal's orchestration/cluster surface is developer-only (Charter §6.3:
//! residents navigate by room, never by infrastructure). ServiceLB status — the
//! LoadBalancer Services exposed on the cluster and how many are pending — joins
//! `ClusterTopology` and `Logs` as a power-user-only card the layout engine drops
//! entirely in Resident mode.

use cave_home_portal::card::Card;

#[test]
fn servicelb_card_is_developer_only() {
    assert!(
        Card::ServiceLb.is_developer_only(),
        "ServiceLB status is infra — must be hidden from residents/mobile"
    );
}

#[test]
fn servicelb_card_is_distinct_from_cluster_topology() {
    assert_ne!(Card::ServiceLb, Card::ClusterTopology);
}

#[test]
fn servicelb_card_groups_with_the_other_developer_cards() {
    // Every developer-only card flags the same way; ServiceLb is one of them.
    for c in [
        Card::ServiceLb,
        Card::ClusterTopology,
        Card::Logs { entity_id: "x".into() },
    ] {
        assert!(c.is_developer_only());
    }
    // …and it is NOT a resident card.
    assert!(!Card::Entity { entity_id: "e".into() }.is_developer_only());
}
