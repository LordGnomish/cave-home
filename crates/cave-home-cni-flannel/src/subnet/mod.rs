// SPDX-License-Identifier: Apache-2.0
//! Subnet allocation + lease management.
//!
//! Upstream parity: `pkg/subnet/` directory.

pub mod clock;
pub mod errors;
pub mod lease;
pub mod manager;
pub mod mem_registry;
pub mod registry;

#[cfg(target_os = "linux")]
pub mod etcd_registry;

pub use clock::{Clock, MockClock, SystemClock};
pub use errors::{Result, SubnetError};
pub use lease::{EventType, Lease, LeaseAttrs, LeaseEvent, Reservation};
pub use manager::{DEFAULT_LEASE_TTL_SECS, LocalManager, SubnetManager};
pub use mem_registry::MemRegistry;
pub use registry::Registry;
