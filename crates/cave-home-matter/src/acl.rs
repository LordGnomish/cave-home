// SPDX-License-Identifier: Apache-2.0
//! Access control list.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/access/AccessControl.cpp
//!
//! Phase 1 ports the in-memory `AccessControl::Check` decision plus
//! the four privilege levels (View / Operate / Manage / Administer)
//! and three auth modes (PASE / CASE / Group).
//!
//! ## UX vocabulary (ADR-007)
//! The user-facing Portal speaks **"Aile rolü"** (family role) with
//! Yönetici / Üye / Misafir / Çocuk; this internal vocabulary is
//! the chip wire model and stays untranslated.

use std::collections::BTreeSet;

use parking_lot::Mutex;

use crate::error::{MatterError, Result};
use crate::fabric::{FabricIndex, NodeId};

/// Privilege levels.
///
/// # Upstream: src/access/Privilege.h::Privilege
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Privilege {
    View = 1,
    /// ProxyView is a Privilege; here we collapse it into View for Phase 1.
    Operate = 3,
    Manage = 4,
    Administer = 5,
}

/// Authentication mode through which a subject is being checked.
///
/// # Upstream: src/access/AuthMode.h::AuthMode
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMode {
    /// PASE — commissioning-mode subject (only valid pre-commit).
    Pase,
    /// CASE — operational session subject.
    Case,
    /// Group — group-key authenticated.
    Group,
}

/// An ACL entry — chip's per-fabric list of who can do what.
///
/// # Upstream: src/access/AccessControl.cpp::Entry
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub fabric_index: FabricIndex,
    pub privilege: Privilege,
    pub auth_mode: AuthMode,
    /// Empty subjects + CASE/Group → wildcard for that auth_mode.
    /// CASE entries require at least one subject (NodeId).
    pub subjects: BTreeSet<NodeId>,
    /// Empty targets → wildcard (any cluster/endpoint).
    pub targets: Vec<Target>,
}

impl Entry {
    /// Spec validation rules from the AclEntry.cpp validate() path.
    ///
    /// # Upstream: src/access/AccessControl.cpp::Entry::Validate
    pub fn validate(&self) -> Result<()> {
        match self.auth_mode {
            AuthMode::Case if self.subjects.is_empty() && self.privilege != Privilege::Administer => {
                Err(MatterError::Fabric(
                    "non-administer CASE entry requires at least one subject".into(),
                ))
            }
            AuthMode::Group if self.subjects.is_empty() => Err(MatterError::Fabric(
                "Group entry requires at least one group id subject".into(),
            )),
            _ => Ok(()),
        }
    }
}

/// Target tuple — restricts an ACL entry to a cluster/endpoint/device-type.
///
/// # Upstream: src/access/AccessControl.cpp::Entry::Target
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Target {
    pub cluster: Option<u32>,
    pub endpoint: Option<u16>,
    pub device_type: Option<u32>,
}

impl Target {
    #[must_use]
    pub fn matches(&self, cluster: u32, endpoint: u16) -> bool {
        match (self.cluster, self.endpoint) {
            (Some(c), _) if c != cluster => false,
            (_, Some(e)) if e != endpoint => false,
            _ => true,
        }
    }
}

/// A subject identity attached to a request being checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubjectDescriptor {
    pub fabric_index: FabricIndex,
    pub auth_mode: AuthMode,
    pub node_id: NodeId,
}

/// A request being checked — (subject, cluster, endpoint, requested privilege).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestPath {
    pub cluster: u32,
    pub endpoint: u16,
}

/// In-memory ACL store.
///
/// # Upstream: src/access/AccessControl.cpp::AccessControl
#[derive(Debug, Default)]
pub struct AccessControl {
    entries: Mutex<Vec<Entry>>,
}

impl AccessControl {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an entry after validation.
    ///
    /// # Upstream: src/access/AccessControl.cpp::AccessControl::CreateEntry
    pub fn create_entry(&self, entry: Entry) -> Result<()> {
        entry.validate()?;
        self.entries.lock().push(entry);
        Ok(())
    }

    /// Snapshot all entries for the named fabric.
    pub fn entries_for_fabric(&self, fabric: FabricIndex) -> Vec<Entry> {
        self.entries
            .lock()
            .iter()
            .filter(|e| e.fabric_index == fabric)
            .cloned()
            .collect()
    }

    /// Number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Decide an authorization request.
    ///
    /// # Upstream: src/access/AccessControl.cpp::AccessControl::Check
    pub fn check(
        &self,
        subject: SubjectDescriptor,
        path: RequestPath,
        required: Privilege,
    ) -> Result<()> {
        let entries = self.entries.lock();
        for e in entries.iter() {
            if e.fabric_index != subject.fabric_index {
                continue;
            }
            if e.auth_mode != subject.auth_mode {
                continue;
            }
            // Subject match.
            let subject_ok = e.subjects.is_empty() || e.subjects.contains(&subject.node_id);
            if !subject_ok {
                continue;
            }
            // Target match — empty list means wildcard.
            let target_ok = e.targets.is_empty()
                || e.targets.iter().any(|t| t.matches(path.cluster, path.endpoint));
            if !target_ok {
                continue;
            }
            // Privilege ladder.
            if e.privilege >= required {
                return Ok(());
            }
        }
        Err(MatterError::AccessDenied)
    }

    /// Clear all entries (admin reset).
    pub fn reset(&self) {
        self.entries.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case_admin(fabric: u8) -> Entry {
        Entry {
            fabric_index: FabricIndex(fabric),
            privilege: Privilege::Administer,
            auth_mode: AuthMode::Case,
            subjects: BTreeSet::new(),
            targets: Vec::new(),
        }
    }

    fn case_view(fabric: u8, node: u64) -> Entry {
        let mut subjects = BTreeSet::new();
        subjects.insert(NodeId(node));
        Entry {
            fabric_index: FabricIndex(fabric),
            privilege: Privilege::View,
            auth_mode: AuthMode::Case,
            subjects,
            targets: Vec::new(),
        }
    }

    fn req(cluster: u32, endpoint: u16) -> RequestPath {
        RequestPath { cluster, endpoint }
    }

    fn subj(fabric: u8, node: u64) -> SubjectDescriptor {
        SubjectDescriptor {
            fabric_index: FabricIndex(fabric),
            auth_mode: AuthMode::Case,
            node_id: NodeId(node),
        }
    }

    /// # Upstream: src/access/tests/TestAccessControl.cpp::TestAclValidate
    #[test]
    fn entry_validate_rejects_no_subjects_for_case() {
        let mut e = case_view(1, 42);
        e.subjects.clear();
        e.privilege = Privilege::Operate;
        assert!(e.validate().is_err());
        // Administer wildcard CASE is allowed.
        assert!(case_admin(1).validate().is_ok());
    }

    /// # Upstream: src/access/tests/TestAccessControl.cpp::TestCheck
    #[test]
    fn check_allows_admin_view() {
        let ac = AccessControl::new();
        ac.create_entry(case_admin(1)).expect("create");
        ac.check(subj(1, 0x42), req(0x0006, 1), Privilege::View)
            .expect("admin can view");
        ac.check(subj(1, 0x42), req(0x0006, 1), Privilege::Operate)
            .expect("admin can operate");
        ac.check(subj(1, 0x42), req(0x0006, 1), Privilege::Administer)
            .expect("admin can administer");
    }

    /// # Upstream: src/access/tests/TestAccessControl.cpp::TestCheck_DenyMissingPrivilege
    #[test]
    fn check_denies_view_only_doing_operate() {
        let ac = AccessControl::new();
        ac.create_entry(case_view(1, 0x42)).expect("create");
        ac.check(subj(1, 0x42), req(0x0006, 1), Privilege::View)
            .expect("view ok");
        let err = ac
            .check(subj(1, 0x42), req(0x0006, 1), Privilege::Operate)
            .expect_err("operate must be denied");
        match err {
            MatterError::AccessDenied => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn check_filters_by_fabric() {
        let ac = AccessControl::new();
        ac.create_entry(case_admin(1)).expect("create");
        let err = ac
            .check(subj(2, 0x42), req(0x0006, 1), Privilege::View)
            .expect_err("other fabric must deny");
        match err {
            MatterError::AccessDenied => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn check_filters_by_target_cluster() {
        let mut e = case_admin(1);
        e.targets.push(Target {
            cluster: Some(0x0006),
            endpoint: None,
            device_type: None,
        });
        let ac = AccessControl::new();
        ac.create_entry(e).expect("create");
        ac.check(subj(1, 0x42), req(0x0006, 1), Privilege::View)
            .expect("matching cluster ok");
        let err = ac
            .check(subj(1, 0x42), req(0x0008, 1), Privilege::View)
            .expect_err("non-matching cluster must deny");
        match err {
            MatterError::AccessDenied => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn entries_for_fabric_filters() {
        let ac = AccessControl::new();
        ac.create_entry(case_admin(1)).expect("create");
        ac.create_entry(case_admin(2)).expect("create");
        assert_eq!(ac.entries_for_fabric(FabricIndex(1)).len(), 1);
        assert_eq!(ac.entries_for_fabric(FabricIndex(2)).len(), 1);
        assert_eq!(ac.entries_for_fabric(FabricIndex(3)).len(), 0);
    }
}
