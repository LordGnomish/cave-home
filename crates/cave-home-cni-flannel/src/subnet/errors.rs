// SPDX-License-Identifier: Apache-2.0
//! Subnet manager error type.
//!
//! Upstream parity: errors are inlined in `pkg/subnet/local_manager.go` as
//! `fmt.Errorf` calls. We surface them as a typed enum so callers can match.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubnetError {
    /// No free subnet remains in the configured `[SubnetMin..SubnetMax]` range.
    #[error("no free subnet available in configured range")]
    SubnetExhausted,

    /// The lease being renewed has already expired (registry returned `None`).
    #[error("lease for {0} expired or never existed")]
    LeaseNotFound(ipnet::Ipv4Net),

    /// Caller-provided subnet is outside the configured network range.
    #[error("subnet {subnet} is not within configured network {network}")]
    SubnetOutOfRange {
        subnet: ipnet::Ipv4Net,
        network: ipnet::Ipv4Net,
    },

    /// A registry-level operation failed (network, etcd, etc.).
    #[error("registry error: {0}")]
    Registry(String),

    /// The configured network/subnet-len combination produces zero candidates.
    #[error("invalid network configuration: {0}")]
    InvalidConfig(String),

    /// Two leases collide on the same subnet (race).
    #[error("subnet {0} already leased")]
    SubnetConflict(ipnet::Ipv4Net),
}

impl SubnetError {
    pub fn registry<S: Into<String>>(msg: S) -> Self {
        Self::Registry(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, SubnetError>;
