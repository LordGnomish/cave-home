// SPDX-License-Identifier: Apache-2.0
//! Fabric (household) table.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/credentials/FabricTable.cpp
//!
//! Hand-port of the FabricTable + FabricInfo + pending-commit
//! machinery. In Matter, a **fabric** is the set of devices an admin
//! commissioner has paired together — cave-home calls this a
//! **"Hane"** ("household") in user-facing UI per ADR-007.
//!
//! Phase 1 stores fabrics in process-local memory; the persistent
//! key/value store binding lands in Phase 1b (`KvsBackend` trait).

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::error::{MatterError, Result};

/// 64-bit Matter Fabric ID.
///
/// # Upstream: src/lib/core/DataModelTypes.h::FabricId
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FabricId(pub u64);

/// 64-bit Matter Node ID.
///
/// # Upstream: src/lib/core/DataModelTypes.h::NodeId
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId(pub u64);

/// Per-fabric index used everywhere in the chip stack to dereference a
/// specific household record.
///
/// # Upstream: src/lib/core/DataModelTypes.h::FabricIndex
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FabricIndex(pub u8);

/// FabricInfo subset used by Phase 1.
///
/// # Upstream: src/credentials/FabricTable.h::FabricInfo
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FabricInfo {
    pub index: FabricIndex,
    pub fabric_id: FabricId,
    pub node_id: NodeId,
    pub vendor_id: u16,
    pub fabric_label: String,
    pub root_ca_public_key: [u8; 32],
    pub icac_public_key: Option<[u8; 32]>,
    pub noc_public_key: [u8; 32],
}

impl FabricInfo {
    /// Validate cross-field consistency.
    pub fn validate(&self) -> Result<()> {
        if self.fabric_id.0 == 0 {
            return Err(MatterError::Fabric("fabric id must not be 0".into()));
        }
        if self.node_id.0 == 0 {
            return Err(MatterError::Fabric("node id must not be 0".into()));
        }
        if self.root_ca_public_key == [0u8; 32] {
            return Err(MatterError::Fabric("root CA public key zero".into()));
        }
        Ok(())
    }
}

/// FabricTable — pending + committed fabrics.
///
/// # Upstream: src/credentials/FabricTable.cpp::FabricTable
#[derive(Debug, Default)]
pub struct FabricTable {
    state: Mutex<FabricTableState>,
}

#[derive(Debug, Default)]
struct FabricTableState {
    committed: BTreeMap<FabricIndex, FabricInfo>,
    pending: Option<FabricInfo>,
    next_index: u8,
}

impl FabricTable {
    /// Construct an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage a new fabric for commit. Equivalent to upstream's
    /// `AddNewPendingFabric` step in NOC processing.
    ///
    /// # Upstream: src/credentials/FabricTable.cpp::FabricTable::AddNewPendingFabric
    pub fn add_pending(&self, mut fabric: FabricInfo) -> Result<FabricIndex> {
        fabric.validate()?;
        let mut s = self.state.lock();
        if s.pending.is_some() {
            return Err(MatterError::IncorrectState(
                "another fabric is already pending".into(),
            ));
        }
        if s.committed
            .values()
            .any(|f| f.fabric_id == fabric.fabric_id && f.node_id == fabric.node_id)
        {
            return Err(MatterError::AlreadyExists(format!(
                "fabric {:?} node {:?} already committed",
                fabric.fabric_id, fabric.node_id
            )));
        }
        s.next_index = s.next_index.saturating_add(1);
        if s.next_index == 0 {
            // FabricIndex = 0 is reserved per chip.
            s.next_index = 1;
        }
        fabric.index = FabricIndex(s.next_index);
        let idx = fabric.index;
        s.pending = Some(fabric);
        Ok(idx)
    }

    /// Promote the pending fabric to committed.
    ///
    /// # Upstream: src/credentials/FabricTable.cpp::FabricTable::CommitPendingFabricData
    pub fn commit_pending(&self) -> Result<FabricIndex> {
        let mut s = self.state.lock();
        let fabric = s.pending.take().ok_or_else(|| {
            MatterError::IncorrectState("no fabric pending; cannot commit".into())
        })?;
        let idx = fabric.index;
        s.committed.insert(idx, fabric);
        Ok(idx)
    }

    /// Abort the pending fabric without committing.
    ///
    /// # Upstream: src/credentials/FabricTable.cpp::FabricTable::RevertPendingFabricData
    pub fn revert_pending(&self) {
        let mut s = self.state.lock();
        s.pending = None;
    }

    /// Delete a committed fabric.
    ///
    /// # Upstream: src/credentials/FabricTable.cpp::FabricTable::Delete
    pub fn delete_fabric(&self, index: FabricIndex) -> Result<FabricInfo> {
        let mut s = self.state.lock();
        s.committed
            .remove(&index)
            .ok_or_else(|| MatterError::NotFound(format!("fabric index {:?}", index)))
    }

    /// Lookup a fabric by index.
    pub fn get(&self, index: FabricIndex) -> Option<FabricInfo> {
        self.state.lock().committed.get(&index).cloned()
    }

    /// Iterate over committed fabrics.
    ///
    /// # Upstream: src/credentials/FabricTable.cpp::FabricTable::FabricIterator
    pub fn iter(&self) -> Vec<FabricInfo> {
        self.state.lock().committed.values().cloned().collect()
    }

    /// Number of committed fabrics.
    pub fn len(&self) -> usize {
        self.state.lock().committed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(fabric: u64, node: u64) -> FabricInfo {
        FabricInfo {
            index: FabricIndex(0),
            fabric_id: FabricId(fabric),
            node_id: NodeId(node),
            vendor_id: 0xFFF1,
            fabric_label: format!("Hane-{fabric}"),
            root_ca_public_key: [1; 32],
            icac_public_key: None,
            noc_public_key: [2; 32],
        }
    }

    /// # Upstream: src/credentials/tests/TestFabricTable.cpp::TestAddNocRoot
    #[test]
    fn add_pending_then_commit_persists_fabric() {
        let table = FabricTable::new();
        let idx = table.add_pending(sample(1, 0x1000_0000_0000_0001)).expect("pending");
        assert_eq!(idx.0, 1, "first fabric index should be 1");
        assert_eq!(table.len(), 0, "still 0 until commit");
        let committed = table.commit_pending().expect("commit");
        assert_eq!(committed, idx);
        assert_eq!(table.len(), 1);
        let got = table.get(idx).expect("get");
        assert_eq!(got.fabric_id, FabricId(1));
    }

    /// # Upstream: src/credentials/tests/TestFabricTable.cpp::TestDelete
    #[test]
    fn delete_fabric_removes_entry() {
        let table = FabricTable::new();
        table.add_pending(sample(1, 0x1000_0000_0000_0001)).expect("pending");
        let idx = table.commit_pending().expect("commit");
        let removed = table.delete_fabric(idx).expect("delete");
        assert_eq!(removed.fabric_id, FabricId(1));
        assert_eq!(table.len(), 0);
        assert!(table.delete_fabric(idx).is_err(), "double-delete must fail");
    }

    /// # Upstream: src/credentials/tests/TestFabricTable.cpp::TestIterator
    #[test]
    fn iterator_visits_committed_fabrics() {
        let table = FabricTable::new();
        for (f, n) in [(1u64, 0x10u64), (2, 0x20), (3, 0x30)] {
            table.add_pending(sample(f, n)).expect("pending");
            table.commit_pending().expect("commit");
        }
        let fabrics = table.iter();
        assert_eq!(fabrics.len(), 3);
        let mut ids: Vec<u64> = fabrics.iter().map(|f| f.fabric_id.0).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn add_pending_rejects_zero_fabric() {
        let table = FabricTable::new();
        let mut bad = sample(0, 1);
        bad.fabric_id = FabricId(0);
        assert!(table.add_pending(bad).is_err());
    }

    #[test]
    fn add_pending_rejects_duplicate_after_commit() {
        let table = FabricTable::new();
        table.add_pending(sample(1, 1)).expect("pending");
        table.commit_pending().expect("commit");
        assert!(table.add_pending(sample(1, 1)).is_err());
    }

    #[test]
    fn revert_pending_drops_pending() {
        let table = FabricTable::new();
        table.add_pending(sample(1, 1)).expect("pending");
        table.revert_pending();
        // Now we can add another.
        table.add_pending(sample(2, 2)).expect("second pending");
    }

    #[test]
    fn add_pending_rejects_concurrent_pending() {
        let table = FabricTable::new();
        table.add_pending(sample(1, 1)).expect("first");
        assert!(table.add_pending(sample(2, 2)).is_err());
    }
}
