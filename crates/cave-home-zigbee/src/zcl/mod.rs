// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Zigbee Cluster Library (ZCL) — frame format + Foundation commands.
//!
//! Implements the slice of the ZCL specification cave-home needs at
//! Phase 1: the frame header (§2.6), the Foundation profile-wide
//! commands (§2.4) used by every cluster, and the data-type encoding
//! (§2.5) for the attribute types we exchange.

pub mod data_type;
pub mod foundation;
pub mod frame;

pub use data_type::{AttributeValue, ZclDataType};
pub use foundation::{
    AttributeRecord, ConfigureReporting, FoundationCommandId, ReadAttributes,
    ReadAttributesResponse, ReportingDirection, WriteAttributes,
};
pub use frame::{Direction, FrameType, ManufacturerCode, ZclFrame, ZclFrameControl};
