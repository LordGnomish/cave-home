// SPDX-License-Identifier: Apache-2.0
//! Re-exports for downstream crates.

pub use crate::acl::{AccessControl, AuthMode, Entry, Privilege};
pub use crate::case::CaseSession;
pub use crate::clusters::color_control::ColorControlClient;
pub use crate::clusters::door_lock::{DoorLockClient, DoorLockState};
pub use crate::clusters::level_control::LevelControlClient;
pub use crate::clusters::network_commissioning::{
    NetworkCommissioningClient, ThreadOperationalDataset,
};
pub use crate::clusters::on_off::OnOffClient;
pub use crate::clusters::thermostat::{SetpointMode, ThermostatClient};
pub use crate::commissioner::{Commissioner, PairedDevice};
pub use crate::controller::Controller;
pub use crate::error::{MatterError, Result};
pub use crate::fabric::{FabricId, FabricInfo, FabricTable, NodeId};
pub use crate::group_key::{GroupDataProvider, GroupId, GroupInfo, GroupKeySet};
pub use crate::ota::{OtaRequestor, OtaState};
pub use crate::pase::PaseSession;
pub use crate::setup_payload::{
    parse_manual_pairing_code, parse_qr_payload, CommissioningFlow,
    RendezvousInformationFlags, SetupPayload,
};
