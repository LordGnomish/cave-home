// SPDX-License-Identifier: Apache-2.0
//! `ImageLocality` — prefer nodes that already have the pod's images cached.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/imagelocality/image_locality.go

use crate::cache::NodeInfo;
use crate::framework::{CycleState, ScorePlugin, Status, MAX_NODE_SCORE};
use crate::types::Pod;

pub struct ImageLocality;

impl ScorePlugin for ImageLocality {
    fn name(&self) -> &'static str {
        "ImageLocality"
    }

    /// Phase 2 implementation: score is the fraction of pod images
    /// already present on the node, scaled to `MAX_NODE_SCORE`.
    /// Upstream's `calculateScoreFromImageWeight` also factors in image
    /// size and the number of nodes that hold the image, but that data
    /// is not yet captured by `NodeStatus` in Phase 2 — see
    /// `[[unmapped]] phase-2b ImageLocality image-size weighting`.
    fn score(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> (i64, Status) {
        let total = pod.spec.containers.len();
        if total == 0 {
            return (0, Status::success());
        }
        let present = pod
            .spec
            .containers
            .iter()
            .filter(|c| !c.image.is_empty() && node.node().has_image(&c.image))
            .count();
        let score = (present as i64).saturating_mul(MAX_NODE_SCORE) / total as i64;
        (score.clamp(0, MAX_NODE_SCORE), Status::success())
    }

    fn weight(&self) -> i64 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Container, Node, Pod};

    fn node_with_images(imgs: &[&str]) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.status.images = imgs.iter().map(|s| (*s).into()).collect();
        NodeInfo::new(n)
    }

    fn pod_with_images(imgs: &[&str]) -> Pod {
        let mut p = Pod::default();
        for img in imgs {
            let mut c = Container::default();
            c.image = (*img).into();
            p.spec.containers.push(c);
        }
        p
    }

    #[test]
    fn node_with_all_images_scores_max() {
        let info = node_with_images(&["a", "b"]);
        let p = pod_with_images(&["a", "b"]);
        let mut s = CycleState::new();
        let (sc, _) = ImageLocality.score(&mut s, &p, &info);
        assert_eq!(sc, MAX_NODE_SCORE);
    }

    #[test]
    fn node_with_no_images_scores_zero() {
        let info = node_with_images(&[]);
        let p = pod_with_images(&["a", "b"]);
        let mut s = CycleState::new();
        let (sc, _) = ImageLocality.score(&mut s, &p, &info);
        assert_eq!(sc, 0);
    }

    #[test]
    fn partial_image_overlap_scores_proportionally() {
        let info = node_with_images(&["a"]);
        let p = pod_with_images(&["a", "b"]);
        let mut s = CycleState::new();
        let (sc, _) = ImageLocality.score(&mut s, &p, &info);
        assert_eq!(sc, MAX_NODE_SCORE / 2);
    }
}
