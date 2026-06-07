// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The clean, grandma-friendly domain model the [`crate::adapter::EnergyProvider`]
//! trait speaks, decoupled from the Fleet API wire DTOs in
//! [`crate::fleet_api::types`].
//!
//! The wire→domain mapping lives here (as `From`/constructor impls), so the
//! transport layer stays a thin shell and the household-facing surfaces (Portal,
//! CLI, voice) only ever see these types.

pub mod battery;
pub mod history;
pub mod operation_mode;
pub mod power_flow;
pub mod site;

pub use battery::BatteryData;
pub use history::{DateRange, HistoryData, HistorySample};
pub use operation_mode::OpMode;
pub use power_flow::PowerFlowData;
pub use site::SiteStatus;

/// The languages cave-home renders household-facing energy labels in
/// (Charter §6.3, ADR-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// German.
    De,
    /// Turkish.
    Tr,
}
