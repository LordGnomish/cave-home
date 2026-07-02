// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! `cave-home-tracker` — a persistent, config-driven upstream **delta tracker**.
//!
//! Instead of dispatching a one-off audit ray every time someone asks "how far
//! along is the K3s port?", this crate makes the answer a standing, reproducible
//! measurement. Given a `tracker.yaml` it will:
//!
//! 1. **poll** — shallow-clone (or update) every tracked upstream
//!    ([`git`], [`config`]);
//! 2. **measure** — count honest source LOC for each upstream and its cave-home
//!    port, run the port's tests, count stubs, and fold the result into a
//!    per-subsystem [`Snapshot`](snapshot::Snapshot) using an
//!    [`honest`](honest) completion formula — no paperwork;
//! 3. **diff** — compare today's snapshot against the previous one
//!    ([`diff`]);
//! 4. **report** — render a daily markdown progress report with per-subsystem
//!    tables, group aggregates and a 30-day text trend ([`report`]);
//! 5. **dashboard** — expose the same numbers as Prometheus metrics on
//!    `:9102/metrics` ([`metrics`], [`dashboard`]).
//!
//! The binary is **generic**: the same executable tracks cave-home or
//! cave-runtime (or anything else) purely by pointing `--config` at a different
//! `tracker.yaml`.

pub mod config;
pub mod dashboard;
pub mod diff;
pub mod error;
pub mod git;
pub mod honest;
pub mod loc;
pub mod measure;
pub mod metrics;
pub mod report;
pub mod snapshot;
pub mod stubs;

pub use config::{Subsystem, TrackerConfig, Upstream, UpstreamRef};
pub use error::{Result, TrackerError};
pub use snapshot::{Snapshot, SubsystemMetric};
