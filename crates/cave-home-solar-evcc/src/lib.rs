// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-solar-evcc` — line-by-line port of the
//! [`evcc-io/evcc`](https://github.com/evcc-io/evcc) loadpoint /
//! site / planner core (Apache-2.0).
//!
//! Upstream: `evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0`
//! (release `0.306.3`), subpath `core/`.
//!
//! # Pillar coverage (Charter §3 / Solar Tier 1)
//!
//! The EVCC port covers four behavioural pillars cave-home needs from
//! the upstream:
//!
//! 1. **Surplus management** — `site` aggregates grid / PV / battery
//!    meters into available surplus power. See [`site`].
//! 2. **EV charge scheduling** — `loadpoint` enforces phase / current
//!    limits, vehicle binding, and charge modes. See [`loadpoint`].
//! 3. **Heat-pump load shifting** — `loadpoint` exposes the same
//!    surplus-aware control surface for non-EV heat-pump loads via
//!    SG-Ready / generic relay backends. See [`loadpoint::Kind`].
//! 4. **Battery management** — `site` chooses when the home battery
//!    discharges into the grid or holds for self-consumption. See
//!    [`site::BatteryMode`].
//!
//! Charge planning lives in [`planner`]; price/CO₂ tariff inputs in
//! [`tariff`]; and the surplus loop tick that ties them all together
//! is [`site::Site::tick`].
//!
//! # Charter §6.3 grandma-friendly UX
//!
//! Public types speak the home-world vocabulary (`Solar`, `Battery`,
//! `EvCharger`, `HeatPump`) — kW, %SoC, plain mode names. Raw Watts,
//! Modbus registers, and SunSpec model IDs stay inside
//! `cave-home-solar-sunspec`, never re-exported here.

#![allow(clippy::module_name_repetitions)]

pub mod error;
pub mod loadpoint;
pub mod planner;
pub mod prioritizer;
pub mod session;
pub mod site;
pub mod tariff;
pub mod tick;

pub use error::{Error, Result};
pub use loadpoint::{ChargeMode, Kind, Loadpoint, LoadpointStatus, MinMaxCurrent, PhaseCount};
pub use planner::{Plan, PlanSlot, Planner};
pub use site::{BatteryMode, GridMeter, PvMeter, ResidualPower, Site, Surplus};
pub use tariff::{Tariff, TariffKind, TariffSample};

/// Upstream provenance string baked into the binary for ops sanity
/// checks (Charter §7 always-latest mandate).
pub const UPSTREAM_PROVENANCE: &str =
    "evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 (release 0.306.3)";
