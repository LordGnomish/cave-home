// SPDX-License-Identifier: Apache-2.0
//! Proxier composition + reconciler loop.
//! Upstream: `pkg/proxy/iptables/proxier.go` (`syncRunner`).

#[allow(clippy::module_inception)]
pub mod proxier;
pub mod reconciler;
