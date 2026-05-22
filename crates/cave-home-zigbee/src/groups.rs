// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Groups cluster (0x0004) — ZCL §3.6.
//!
//! A *Group* is a multicast address (ZBDB §10.3) shared by one or more
//! device endpoints. The headline persona experiences groups as the
//! "Salon Lambaları" (Living-room lights) tile in the Portal — they
//! never see the cluster ID.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Result, ZigbeeError};

/// Groups cluster identifier (ZCL §3.6.1).
pub const GROUPS_CLUSTER_ID: u16 = 0x0004;

/// Command identifiers — ZCL §3.6.2.1.
pub mod command_id {
    /// Add Group (0x00).
    pub const ADD: u8 = 0x00;
    /// View Group (0x01).
    pub const VIEW: u8 = 0x01;
    /// Get Group Membership (0x02).
    pub const GET_MEMBERSHIP: u8 = 0x02;
    /// Remove Group (0x03).
    pub const REMOVE: u8 = 0x03;
    /// Remove All Groups (0x04).
    pub const REMOVE_ALL: u8 = 0x04;
    /// Add Group If Identifying (0x05).
    pub const ADD_IF_IDENTIFYING: u8 = 0x05;
}

/// A single group — a 16-bit ID + user-facing name (Charter §6.3: "Salon").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Group {
    /// Group ID (1..=0xfff7 per ZCL §3.6.1).
    pub id: u16,
    /// Human-readable name — surfaced as "Grup" in the Portal.
    pub name: String,
    /// IEEE addresses of devices that belong to this group.
    pub members: BTreeSet<u64>,
}

impl Group {
    /// Construct an empty group.
    #[must_use]
    pub fn new(id: u16, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            members: BTreeSet::new(),
        }
    }
}

/// In-memory groups table — Phase 1 backing store for the cluster.
///
/// The table is persisted by `cave-home-orchestration` (out of scope
/// for this crate). Phase 1 only needs the in-memory representation +
/// the Add / View / Remove / Get-Membership / Remove-All operations.
#[derive(Clone, Debug, Default)]
pub struct GroupsCluster {
    groups: BTreeMap<u16, Group>,
}

impl GroupsCluster {
    /// Empty cluster.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a device (`ieee`) to group `id`, creating the group if needed.
    ///
    /// `name` is used only when the group is freshly created; for
    /// existing groups it's ignored. ZCL §3.6.2.2.1 Add Group response
    /// returns `SUCCESS` even when the device was already a member.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Network`] if `id` is reserved (0x0000 or
    /// 0xfff8..=0xffff per ZCL §3.6.1).
    pub fn add(&mut self, id: u16, name: &str, ieee: u64) -> Result<()> {
        if id == 0x0000 || id >= 0xfff8 {
            return Err(ZigbeeError::Network(format!(
                "reserved group id 0x{id:04x}"
            )));
        }
        let g = self.groups.entry(id).or_insert_with(|| Group::new(id, name));
        g.members.insert(ieee);
        Ok(())
    }

    /// Remove `ieee` from group `id`. If the group becomes empty the
    /// table also drops the group entry (matches Z2M-class UX where
    /// empty groups disappear from the user view).
    pub fn remove(&mut self, id: u16, ieee: u64) -> bool {
        if let Some(g) = self.groups.get_mut(&id) {
            let removed = g.members.remove(&ieee);
            if g.members.is_empty() {
                self.groups.remove(&id);
            }
            removed
        } else {
            false
        }
    }

    /// Remove all groups (`ieee` is dropped from every group). Returns
    /// the number of group memberships dropped.
    pub fn remove_all_for(&mut self, ieee: u64) -> usize {
        let mut dropped = 0usize;
        let mut empty: Vec<u16> = Vec::new();
        for (id, g) in &mut self.groups {
            if g.members.remove(&ieee) {
                dropped += 1;
            }
            if g.members.is_empty() {
                empty.push(*id);
            }
        }
        for id in empty {
            self.groups.remove(&id);
        }
        dropped
    }

    /// View group (read-only). Returns `None` if the group is unknown.
    #[must_use]
    pub fn view(&self, id: u16) -> Option<&Group> {
        self.groups.get(&id)
    }

    /// Membership snapshot for `ieee` (group ids only). Sorted.
    #[must_use]
    pub fn membership_of(&self, ieee: u64) -> Vec<u16> {
        self.groups
            .values()
            .filter(|g| g.members.contains(&ieee))
            .map(|g| g.id)
            .collect()
    }

    /// All groups (snapshot).
    #[must_use]
    pub fn list(&self) -> Vec<Group> {
        self.groups.values().cloned().collect()
    }

    /// Rename a group (Portal "Grup yeniden adlandır" surface).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Unknown`] if `id` doesn't exist.
    pub fn rename(&mut self, id: u16, new_name: impl Into<String>) -> Result<()> {
        let g = self.groups.get_mut(&id).ok_or_else(|| ZigbeeError::Unknown {
            kind: "group",
            id: format!("0x{id:04x}"),
        })?;
        g.name = new_name.into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_creates_group_and_member() {
        let mut c = GroupsCluster::new();
        c.add(1, "Salon Lambaları", 0xaaaa).unwrap();
        let g = c.view(1).unwrap();
        assert_eq!(g.name, "Salon Lambaları");
        assert!(g.members.contains(&0xaaaa));
    }

    #[test]
    fn add_to_existing_group_keeps_name() {
        let mut c = GroupsCluster::new();
        c.add(1, "Salon", 0xaaaa).unwrap();
        c.add(1, "Other", 0xbbbb).unwrap();
        assert_eq!(c.view(1).unwrap().name, "Salon");
        assert_eq!(c.view(1).unwrap().members.len(), 2);
    }

    #[test]
    fn add_rejects_reserved_ids() {
        let mut c = GroupsCluster::new();
        assert!(c.add(0x0000, "x", 1).is_err());
        assert!(c.add(0xfff8, "x", 1).is_err());
        assert!(c.add(0xffff, "x", 1).is_err());
    }

    #[test]
    fn remove_drops_member_and_empty_group() {
        let mut c = GroupsCluster::new();
        c.add(1, "Salon", 0xaaaa).unwrap();
        assert!(c.remove(1, 0xaaaa));
        assert!(c.view(1).is_none());
    }

    #[test]
    fn remove_unknown_member_returns_false() {
        let mut c = GroupsCluster::new();
        c.add(1, "x", 0xaaaa).unwrap();
        assert!(!c.remove(1, 0xbbbb));
    }

    #[test]
    fn remove_all_for_drops_membership_in_every_group() {
        let mut c = GroupsCluster::new();
        c.add(1, "a", 0xaaaa).unwrap();
        c.add(2, "b", 0xaaaa).unwrap();
        c.add(2, "b", 0xbbbb).unwrap();
        assert_eq!(c.remove_all_for(0xaaaa), 2);
        // Group 1 is now empty → gone.
        assert!(c.view(1).is_none());
        // Group 2 still has bbbb.
        assert_eq!(c.view(2).unwrap().members.len(), 1);
    }

    #[test]
    fn membership_returns_sorted_ids() {
        let mut c = GroupsCluster::new();
        c.add(3, "c", 0xaaaa).unwrap();
        c.add(1, "a", 0xaaaa).unwrap();
        c.add(2, "b", 0xaaaa).unwrap();
        assert_eq!(c.membership_of(0xaaaa), vec![1, 2, 3]);
    }

    #[test]
    fn rename_known_group_ok() {
        let mut c = GroupsCluster::new();
        c.add(1, "x", 0xaaaa).unwrap();
        c.rename(1, "Salon").unwrap();
        assert_eq!(c.view(1).unwrap().name, "Salon");
    }

    #[test]
    fn rename_unknown_group_errors() {
        let mut c = GroupsCluster::new();
        assert!(c.rename(99, "Salon").is_err());
    }
}
