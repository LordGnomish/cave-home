// SPDX-License-Identifier: Apache-2.0
//! Image resolver / pull — line-by-line port of containerd's
//! `core/remotes/docker/{resolver,auth}` packages. Phase 1: HTTPS + sha256
//! manifest/blob fetch with bearer-token challenge handling. Multi-arch
//! manifest lists pick `linux/amd64` (Phase 1b: arch detection).

pub mod auth;
pub mod resolver;
pub mod unpacker;

pub use resolver::{Reference, ResolveError, Resolved, Resolver};
