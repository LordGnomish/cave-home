// SPDX-License-Identifier: Apache-2.0
//! One module per Phase 2 controller.
//!
//! Every module here ports the homonymous sub-package of
//! `kubernetes/kubernetes` `pkg/controller/` (v1.36.1).

pub mod cronjob;
pub mod daemonset;
pub mod deployment;
pub mod garbage_collector;
pub mod job;
pub mod namespace;
pub mod node;
pub mod replicaset;
pub mod serviceaccount;
pub mod statefulset;
