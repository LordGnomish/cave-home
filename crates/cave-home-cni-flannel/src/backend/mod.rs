// SPDX-License-Identifier: Apache-2.0
//! Backend trait + implementations.
//!
//! Upstream parity: `pkg/backend/` directory. Phase 1 ships VXLAN only.

pub mod trait_def;
pub mod vxlan;

pub use trait_def::{Backend, BackendError, BackendNetwork};
