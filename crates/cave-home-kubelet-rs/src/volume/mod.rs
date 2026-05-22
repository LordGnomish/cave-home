// SPDX-License-Identifier: Apache-2.0
//! Volume manager (emptyDir + hostPath only — Phase 1).
//!
//! Hand-port of `pkg/kubelet/volumemanager/` + `pkg/volume/{emptydir,hostpath}`.
//!
//! ConfigMap / Secret / Projected / CSI / PVC volumes are deferred to
//! Phase 1b (recorded in `parity.manifest.toml`).

pub mod actual;
pub mod desired;
pub mod emptydir;
pub mod hostpath;
pub mod plugin;
pub mod reconciler;

pub use actual::ActualStateOfWorld;
pub use desired::DesiredStateOfWorld;
pub use plugin::{VolumeError, VolumePlugin, VolumeResult};
pub use reconciler::Reconciler;
