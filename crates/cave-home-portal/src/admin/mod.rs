// SPDX-License-Identifier: Apache-2.0
//! Portal admin sub-modules. Each capability lands its own module
//! here as it gets wired into the Lovelace-class dashboard.

pub mod apiserver;
pub mod cni;
pub mod containers;
pub mod controller_manager;
pub mod hue;
pub mod kubelet;
pub mod proxy;
pub mod scheduler;
pub mod solar;
pub mod unifi;
