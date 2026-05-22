// SPDX-License-Identifier: Apache-2.0
//! `Reconciler` — drives DesiredStateOfWorld -> ActualStateOfWorld.
//!
//! Hand-port of `pkg/kubelet/volumemanager/reconciler/reconciler.go`
//! (`reconcile()` loop body, v1.36.1).
//!
//! One pass:
//! 1. For every (pod_uid, volume) in DSW that is not yet in ASW, locate a
//!    plugin that supports it, call `set_up`, and mark mounted.
//! 2. For every (pod_uid, volume) in ASW that is no longer in DSW, locate a
//!    plugin that supports it, call `tear_down`, and unmark mounted.

use std::collections::HashSet;
use std::sync::Arc;

use super::actual::{ActualStateOfWorld, MountedVolume};
use super::desired::DesiredStateOfWorld;
use super::plugin::{VolumeError, VolumePlugin, VolumeResult};
use crate::api::{PodUid, Volume};

pub struct Reconciler {
    plugins: Vec<Arc<dyn VolumePlugin>>,
    desired: Arc<DesiredStateOfWorld>,
    actual: Arc<ActualStateOfWorld>,
}

impl Reconciler {
    pub fn new(
        plugins: Vec<Arc<dyn VolumePlugin>>,
        desired: Arc<DesiredStateOfWorld>,
        actual: Arc<ActualStateOfWorld>,
    ) -> Self {
        Self {
            plugins,
            desired,
            actual,
        }
    }

    /// Find the plugin that can handle `volume`.
    fn find_plugin(&self, volume: &Volume) -> Option<Arc<dyn VolumePlugin>> {
        self.plugins.iter().find(|p| p.can_support(volume)).cloned()
    }

    /// One reconcile pass.
    pub async fn reconcile_once(&self) -> VolumeResult<()> {
        let desired = self.desired.snapshot();
        let desired_keys: HashSet<(PodUid, String)> = desired
            .iter()
            .map(|(uid, v)| (uid.clone(), v.name.clone()))
            .collect();

        // Mount missing volumes.
        for (uid, volume) in &desired {
            if self.actual.is_mounted(uid, &volume.name) {
                continue;
            }
            let Some(plugin) = self.find_plugin(volume) else {
                return Err(VolumeError::Unsupported("no plugin for volume"));
            };
            let host_path = plugin.set_up(uid, volume).await?;
            self.actual.add_mounted(MountedVolume {
                pod_uid: uid.clone(),
                volume_name: volume.name.clone(),
                host_path,
            });
        }

        // Unmount surplus volumes.
        let actual_snapshot = self.actual.snapshot();
        for m in actual_snapshot {
            if desired_keys.contains(&(m.pod_uid.clone(), m.volume_name.clone())) {
                continue;
            }
            // Synthesise a Volume just for the tear-down call: the only
            // field a plugin uses is `name`, plus the source variant tag,
            // which a `find_plugin` lookup needs. Since we know the path is
            // the on-disk one, we can match by inspecting which plugin's
            // host_path layout would produce it; in Phase 1 the only plugin
            // that owns a tear-down side-effect is EmptyDirPlugin, so we
            // always try plugins in order and take the first that succeeds.
            for plugin in &self.plugins {
                let synth = Volume {
                    name: m.volume_name.clone(),
                    source: crate::api::VolumeSource::EmptyDir(
                        crate::api::EmptyDirVolumeSource::default(),
                    ),
                };
                if plugin.can_support(&synth) {
                    plugin.tear_down(&m.pod_uid, &synth).await?;
                    break;
                }
            }
            self.actual.remove_mounted(&m.pod_uid, &m.volume_name);
        }
        Ok(())
    }
}
