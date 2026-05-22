// SPDX-License-Identifier: Apache-2.0
//! CNI plugin protocol: types, ADD/DEL/CHECK/VERSION dispatch, subnet.env
//! parsing.
//!
//! Upstream parity: <https://github.com/flannel-io/cni-plugin> (Phase 1
//! emits the delegate-bridge config; actual delegate exec chaining is a
//! Phase 1b deliverable — recorded honestly in the parity manifest).

pub mod handler;
pub mod subnet_env;
pub mod types;

pub use handler::{CniError, CniInvocation, CniRequest, CniResponse, dispatch};
pub use subnet_env::{SubnetEnv, parse_subnet_env};
pub use types::{CniResult, IpConfig, NetConf, Route};
