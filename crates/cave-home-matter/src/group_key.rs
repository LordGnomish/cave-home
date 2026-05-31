// SPDX-License-Identifier: Apache-2.0
//! Group Data Provider — multicast group keys + group info.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/credentials/GroupDataProvider.cpp
//!
//! Phase 1 ports the in-memory variant used by chip's unit tests.
//! Persistent KVS backing is Phase 1b.

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::error::{MatterError, Result};
use crate::fabric::FabricIndex;

/// 16-bit Matter Group ID.
///
/// # Upstream: src/lib/core/DataModelTypes.h::GroupId
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GroupId(pub u16);

/// 16-bit keyset id local to a fabric.
///
/// # Upstream: src/credentials/GroupDataProvider.h::KeysetId
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeysetId(pub u16);

/// Per-fabric group info entry.
///
/// # Upstream: src/credentials/GroupDataProvider.h::GroupInfo
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupInfo {
    pub fabric: FabricIndex,
    pub group: GroupId,
    pub name: String,
}

/// Group key — `TrustFirst` or `Cache & Sync` security policy with an
/// epoch start time + 16-byte key.
///
/// # Upstream: src/credentials/GroupDataProvider.h::KeySet::EpochKey
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochKey {
    pub epoch_start_micros: u64,
    pub key: [u8; 16],
}

/// A KeySet — up to 3 epoch keys + policy.
///
/// # Upstream: src/credentials/GroupDataProvider.h::KeySet
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupKeySet {
    pub fabric: FabricIndex,
    pub keyset_id: KeysetId,
    pub policy: GroupKeySecurityPolicy,
    pub epoch_keys: Vec<EpochKey>,
}

/// Security policy.
///
/// # Upstream: src/credentials/GroupDataProvider.h::SecurityPolicy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupKeySecurityPolicy {
    TrustFirst,
    CacheAndSync,
}

impl GroupKeySet {
    pub fn validate(&self) -> Result<()> {
        if self.epoch_keys.is_empty() {
            return Err(MatterError::InvalidArgument(
                "GroupKeySet needs at least one epoch key".into(),
            ));
        }
        if self.epoch_keys.len() > 3 {
            return Err(MatterError::InvalidArgument(
                "GroupKeySet allows at most 3 epoch keys".into(),
            ));
        }
        Ok(())
    }
}

/// In-memory group data provider.
///
/// # Upstream: src/credentials/GroupDataProvider.cpp::GroupDataProviderImpl
#[derive(Debug, Default)]
pub struct GroupDataProvider {
    state: Mutex<GroupDataProviderState>,
}

#[derive(Debug, Default)]
struct GroupDataProviderState {
    groups: BTreeMap<(FabricIndex, GroupId), GroupInfo>,
    keysets: BTreeMap<(FabricIndex, KeysetId), GroupKeySet>,
}

impl GroupDataProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set / overwrite a group info row.
    ///
    /// # Upstream: src/credentials/GroupDataProvider.cpp::SetGroupInfo
    pub fn set_group_info(&self, info: GroupInfo) -> Result<()> {
        if info.name.len() > 16 {
            return Err(MatterError::InvalidArgument(
                "group name must be <= 16 bytes".into(),
            ));
        }
        self.state
            .lock()
            .groups
            .insert((info.fabric, info.group), info);
        Ok(())
    }

    /// Get group info.
    pub fn get_group_info(&self, fabric: FabricIndex, group: GroupId) -> Option<GroupInfo> {
        self.state.lock().groups.get(&(fabric, group)).cloned()
    }

    /// Remove group info.
    pub fn remove_group_info(&self, fabric: FabricIndex, group: GroupId) -> Result<GroupInfo> {
        self.state
            .lock()
            .groups
            .remove(&(fabric, group))
            .ok_or_else(|| MatterError::NotFound(format!("group {:?} on fabric {:?}", group, fabric)))
    }

    /// Set / overwrite a key set.
    ///
    /// # Upstream: src/credentials/GroupDataProvider.cpp::SetKeySet
    pub fn set_group_key(&self, ks: GroupKeySet) -> Result<()> {
        ks.validate()?;
        self.state
            .lock()
            .keysets
            .insert((ks.fabric, ks.keyset_id), ks);
        Ok(())
    }

    /// Get a key set.
    pub fn get_group_key(&self, fabric: FabricIndex, keyset: KeysetId) -> Option<GroupKeySet> {
        self.state.lock().keysets.get(&(fabric, keyset)).cloned()
    }

    /// Iterate all group infos on a fabric.
    pub fn iter_groups(&self, fabric: FabricIndex) -> Vec<GroupInfo> {
        self.state
            .lock()
            .groups
            .iter()
            .filter(|((f, _), _)| *f == fabric)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Iterate all keysets on a fabric.
    pub fn iter_keysets(&self, fabric: FabricIndex) -> Vec<GroupKeySet> {
        self.state
            .lock()
            .keysets
            .iter()
            .filter(|((f, _), _)| *f == fabric)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Upstream: src/credentials/tests/TestGroupDataProvider.cpp::TestKeySets
    #[test]
    fn set_and_get_group_keyset_round_trips() {
        let p = GroupDataProvider::new();
        let ks = GroupKeySet {
            fabric: FabricIndex(1),
            keyset_id: KeysetId(42),
            policy: GroupKeySecurityPolicy::TrustFirst,
            epoch_keys: vec![EpochKey {
                epoch_start_micros: 1,
                key: [0xAA; 16],
            }],
        };
        p.set_group_key(ks.clone()).expect("set");
        let got = p.get_group_key(FabricIndex(1), KeysetId(42)).expect("get");
        assert_eq!(got, ks);
    }

    /// # Upstream: src/credentials/tests/TestGroupDataProvider.cpp::TestGroupInfo
    #[test]
    fn set_and_get_group_info_round_trips() {
        let p = GroupDataProvider::new();
        let info = GroupInfo {
            fabric: FabricIndex(1),
            group: GroupId(0x0005),
            name: "Salon".into(),
        };
        p.set_group_info(info.clone()).expect("set");
        assert_eq!(
            p.get_group_info(FabricIndex(1), GroupId(0x0005)),
            Some(info)
        );
    }

    #[test]
    fn rejects_empty_keysets() {
        let p = GroupDataProvider::new();
        let ks = GroupKeySet {
            fabric: FabricIndex(1),
            keyset_id: KeysetId(1),
            policy: GroupKeySecurityPolicy::TrustFirst,
            epoch_keys: vec![],
        };
        assert!(p.set_group_key(ks).is_err());
    }

    #[test]
    fn rejects_long_name() {
        let p = GroupDataProvider::new();
        let info = GroupInfo {
            fabric: FabricIndex(1),
            group: GroupId(1),
            name: "abcdefghijklmnopqrstuvwxyz".into(),
        };
        assert!(p.set_group_info(info).is_err());
    }

    #[test]
    fn iter_filters_by_fabric() {
        let p = GroupDataProvider::new();
        for (f, g, n) in [
            (1u8, 0x10u16, "Salon"),
            (1, 0x20, "Mutfak"),
            (2, 0x10, "Yatak"),
        ] {
            p.set_group_info(GroupInfo {
                fabric: FabricIndex(f),
                group: GroupId(g),
                name: n.into(),
            })
            .expect("set");
        }
        assert_eq!(p.iter_groups(FabricIndex(1)).len(), 2);
        assert_eq!(p.iter_groups(FabricIndex(2)).len(), 1);
    }

    #[test]
    fn remove_returns_not_found_for_missing() {
        let p = GroupDataProvider::new();
        assert!(p
            .remove_group_info(FabricIndex(99), GroupId(1))
            .is_err());
    }
}
