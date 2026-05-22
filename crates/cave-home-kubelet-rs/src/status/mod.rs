// SPDX-License-Identifier: Apache-2.0
//! Pod status manager.
//!
//! Hand-port of `pkg/kubelet/status/status_manager.go` (v1.36.1).

pub mod manager;
pub mod sink;

pub use manager::{PodStatusManager, StatusManagerError};
pub use sink::{MockStatusSink, StatusSink, StatusSinkError};
