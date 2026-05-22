// SPDX-License-Identifier: Apache-2.0
//! `VolumeRestrictions` — basic PV / single-writer PVC rules.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/volumerestrictions/volume_restrictions.go
//!
//! Phase 2 enforces only the upstream "single-writer PVC" rule —
//! two pods on the same node cannot both mount the same RWO PVC.
//! Storage-class-specific anti-affinity, CSI driver detail, and
//! attach-limit checks are deferred.

use std::collections::HashSet;

use crate::cache::NodeInfo;
use crate::framework::{CycleState, FilterPlugin, Status};
use crate::types::{Pod, VolumeSource};

pub struct VolumeRestrictions;

impl FilterPlugin for VolumeRestrictions {
    fn name(&self) -> &'static str {
        "VolumeRestrictions"
    }

    fn filter(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status {
        let pod_claims: HashSet<&str> = pod
            .spec
            .volumes
            .iter()
            .filter_map(|v| match &v.source {
                VolumeSource::PersistentVolumeClaim(pvc) if !pvc.read_only => {
                    Some(pvc.claim_name.as_str())
                }
                _ => None,
            })
            .collect();

        if pod_claims.is_empty() {
            return Status::success();
        }

        for existing in node.pods() {
            for v in &existing.spec.volumes {
                if let VolumeSource::PersistentVolumeClaim(pvc) = &v.source {
                    if !pvc.read_only && pod_claims.contains(pvc.claim_name.as_str()) {
                        return Status::unschedulable(
                            self.name(),
                            format!(
                                "PVC {} is already used by pod {} on this node",
                                pvc.claim_name,
                                existing.full_name(),
                            ),
                        );
                    }
                }
            }
        }
        Status::success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Node, ObjectMeta, Pod, PvcSource, Volume, VolumeSource};

    fn node() -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        NodeInfo::new(n)
    }

    fn pvc_pod(name: &str, claim: &str, ro: bool) -> Pod {
        let mut p = Pod::default();
        p.metadata = ObjectMeta {
            namespace: "default".into(),
            name: name.into(),
            uid: name.into(),
            ..Default::default()
        };
        p.spec.volumes.push(Volume {
            name: "v".into(),
            source: VolumeSource::PersistentVolumeClaim(PvcSource {
                claim_name: claim.into(),
                read_only: ro,
            }),
        });
        p
    }

    #[test]
    fn pod_without_pvc_passes() {
        let info = node();
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(VolumeRestrictions.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn rwo_pvc_already_in_use_blocks_node() {
        let mut info = node();
        info.add_pod(pvc_pod("alpha", "claim-a", false));
        let p = pvc_pod("beta", "claim-a", false);
        let mut s = CycleState::new();
        let st = VolumeRestrictions.filter(&mut s, &p, &info);
        assert!(!st.is_success());
    }

    #[test]
    fn read_only_pvc_can_be_shared() {
        let mut info = node();
        info.add_pod(pvc_pod("alpha", "claim-a", true));
        let p = pvc_pod("beta", "claim-a", true);
        let mut s = CycleState::new();
        assert!(VolumeRestrictions.filter(&mut s, &p, &info).is_success());
    }
}
