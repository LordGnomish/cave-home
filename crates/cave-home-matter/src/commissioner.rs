// SPDX-License-Identifier: Apache-2.0
//! DeviceCommissioner — the commissioner-side state machine.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/controller/CHIPDeviceController.cpp
//!
//! The commissioner walks a paired device through:
//! 1. Setup payload decode (QR or manual code).
//! 2. PASE (Spake2+) over BLE.
//! 3. AddNOC / FabricTable commit.
//! 4. NetworkCommissioning (Thread / Wi-Fi).
//! 5. CASE handshake on the operational network.
//! 6. Per-cluster onboarding events.
//!
//! Phase 1 wires the steps together against the in-memory transports
//! and crypto modules. Each step is a separately-tested unit; the
//! end-to-end `pair_device` integration test sequences them.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::acl::{AccessControl, AuthMode, Entry, Privilege};
use crate::case::{CaseSession, OperationalCredentials};
use crate::clusters::network_commissioning::{
    NetworkCommissioningClient, ThreadOperationalDataset,
};
use crate::error::{MatterError, Result};
use crate::fabric::{FabricId, FabricIndex, FabricInfo, FabricTable, NodeId};
use crate::pase::{PaseSession, PbkdfParameters};
use crate::setup_payload::{parse_manual_pairing_code, parse_qr_payload, SetupPayload, QR_CODE_PREFIX};

/// A device that has been successfully paired.
///
/// In the UI ADR-007 calls this **"Cihaz"**; the type keeps the chip
/// vocabulary for parity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairedDevice {
    pub fabric_index: FabricIndex,
    pub node_id: NodeId,
    pub vendor_id: u16,
    pub product_id: u16,
    pub fabric_label: String,
}

/// Configuration for a commissioner.
#[derive(Clone, Debug)]
pub struct CommissionerConfig {
    pub admin_fabric_id: FabricId,
    pub admin_node_id: NodeId,
    pub admin_vendor_id: u16,
    pub admin_fabric_label: String,
    pub admin_root_ca_public_key: [u8; 32],
    pub admin_noc_public_key: [u8; 32],
    pub default_thread_dataset: Option<ThreadOperationalDataset>,
}

/// DeviceCommissioner.
///
/// # Upstream: src/controller/CHIPDeviceController.cpp::DeviceCommissioner
pub struct Commissioner {
    cfg: CommissionerConfig,
    fabrics: Arc<FabricTable>,
    acl: Arc<AccessControl>,
    network: Arc<NetworkCommissioningClient>,
    paired: Mutex<Vec<PairedDevice>>,
    next_node_id_inc: Mutex<u64>,
}

impl Commissioner {
    /// Build a commissioner.
    pub fn new(
        cfg: CommissionerConfig,
        fabrics: Arc<FabricTable>,
        acl: Arc<AccessControl>,
        network: Arc<NetworkCommissioningClient>,
    ) -> Self {
        Self {
            cfg,
            fabrics,
            acl,
            network,
            paired: Mutex::new(Vec::new()),
            next_node_id_inc: Mutex::new(0),
        }
    }

    /// Parse either a QR or manual pairing code.
    ///
    /// # Upstream: src/controller/CHIPDeviceController.cpp::DeviceCommissioner::DecodeSetupPayload
    pub fn decode_setup_payload(code: &str) -> Result<SetupPayload> {
        if code.starts_with(QR_CODE_PREFIX) {
            parse_qr_payload(code)
        } else {
            parse_manual_pairing_code(code)
        }
    }

    /// Pair a device. Drives:
    /// PASE → AddNOC → NetworkCommissioning → CASE → ACL provisioning.
    ///
    /// `device_pase` is the device-side counterpart needed so the
    /// in-process Phase 1 handshake can complete; in the production
    /// path it'd be replaced with the BLE-tunnelled remote half.
    ///
    /// # Upstream: src/controller/CHIPDeviceController.cpp::DeviceCommissioner::PairDevice
    pub fn pair_device(
        &self,
        setup_payload: &SetupPayload,
        mut device_pase: PaseSession,
    ) -> Result<PairedDevice> {
        // 1. PASE.
        let params = PbkdfParameters {
            iterations: 10_000,
            salt: vec![0xAB; 16],
        };
        let mut commissioner_pase =
            PaseSession::new_initiator(setup_payload.passcode, params.clone())?;
        device_pase.wait_for_establishment()?;
        commissioner_pase.pair(&mut device_pase)?;

        // 2. Assign the device a node id and add the fabric (pending then commit).
        let device_node_id = self.allocate_node_id();
        let fabric = FabricInfo {
            index: FabricIndex(0),
            fabric_id: self.cfg.admin_fabric_id,
            node_id: device_node_id,
            vendor_id: setup_payload.vendor_id,
            fabric_label: self.cfg.admin_fabric_label.clone(),
            root_ca_public_key: self.cfg.admin_root_ca_public_key,
            icac_public_key: None,
            noc_public_key: derive_device_noc(self.cfg.admin_noc_public_key, device_node_id),
        };
        let pending = self.fabrics.add_pending(fabric.clone());
        let fabric_index = match pending {
            Ok(idx) => match self.fabrics.commit_pending() {
                Ok(committed) => committed,
                Err(e) => {
                    self.fabrics.revert_pending();
                    return Err(e);
                }
            },
            Err(e) => return Err(e),
        };

        // 3. NetworkCommissioning — Thread is the credential-required path.
        if let Some(dataset) = &self.cfg.default_thread_dataset {
            self.network
                .add_thread_network(device_node_id, dataset.clone())?;
            self.network.connect_network(device_node_id)?;
        }

        // 4. CASE on the operational network.
        let initiator_creds = OperationalCredentials {
            fabric_id: self.cfg.admin_fabric_id,
            node_id: self.cfg.admin_node_id,
            noc_public_key: self.cfg.admin_noc_public_key,
            root_ca_public_key: self.cfg.admin_root_ca_public_key,
        };
        let device_creds = OperationalCredentials {
            fabric_id: self.cfg.admin_fabric_id,
            node_id: device_node_id,
            noc_public_key: fabric.noc_public_key,
            root_ca_public_key: self.cfg.admin_root_ca_public_key,
        };
        let mut initiator = CaseSession::new_initiator(initiator_creds.clone())?;
        let mut responder = CaseSession::new_responder(device_creds)?;
        let s1 = initiator.send_sigma1();
        let s2 = responder.handle_sigma1(&s1)?;
        let s3 = initiator.handle_sigma2(&s2)?;
        responder.handle_sigma3(&s3, &initiator_creds.root_ca_public_key)?;
        initiator.finalize_initiator()?;

        // 5. Provision an ACL entry granting the commissioner Administer.
        self.acl.create_entry(Entry {
            fabric_index,
            privilege: Privilege::Administer,
            auth_mode: AuthMode::Case,
            subjects: std::iter::once(self.cfg.admin_node_id).collect(),
            targets: Vec::new(),
        })?;

        let paired = PairedDevice {
            fabric_index,
            node_id: device_node_id,
            vendor_id: setup_payload.vendor_id,
            product_id: setup_payload.product_id,
            fabric_label: self.cfg.admin_fabric_label.clone(),
        };
        self.paired.lock().push(paired.clone());
        Ok(paired)
    }

    /// Unpair a device — delete the fabric row.
    ///
    /// # Upstream: src/controller/CHIPDeviceController.cpp::DeviceCommissioner::UnpairDevice
    pub fn unpair_device(&self, node_id: NodeId) -> Result<PairedDevice> {
        let mut paired = self.paired.lock();
        let pos = paired
            .iter()
            .position(|p| p.node_id == node_id)
            .ok_or_else(|| MatterError::NotFound(format!("paired device {:?}", node_id)))?;
        let device = paired.remove(pos);
        self.fabrics.delete_fabric(device.fabric_index)?;
        Ok(device)
    }

    /// Read the list of paired devices — used by the Portal admin page.
    pub fn list_paired(&self) -> Vec<PairedDevice> {
        self.paired.lock().clone()
    }

    fn allocate_node_id(&self) -> NodeId {
        let mut n = self.next_node_id_inc.lock();
        *n = n.checked_add(1).expect("node id counter overflow");
        NodeId(0x1000_0000_0000_0000 | *n)
    }
}

fn derive_device_noc(admin_pk: [u8; 32], node: NodeId) -> [u8; 32] {
    let mut out = admin_pk;
    let bytes = node.0.to_be_bytes();
    for (i, b) in bytes.iter().enumerate() {
        out[i] ^= *b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(default_thread: bool) -> CommissionerConfig {
        let dataset = if default_thread {
            Some(ThreadOperationalDataset {
                network_name: "cave-home".into(),
                extended_pan_id: [1, 2, 3, 4, 5, 6, 7, 8],
                master_key: [0xAA; 16],
                channel: 15,
                pan_id: 0x1234,
            })
        } else {
            None
        };
        CommissionerConfig {
            admin_fabric_id: FabricId(1),
            admin_node_id: NodeId(0x1000_0000_0000_0001),
            admin_vendor_id: 0xFFF1,
            admin_fabric_label: "Cave Home".into(),
            admin_root_ca_public_key: [0xCC; 32],
            admin_noc_public_key: [0xDD; 32],
            default_thread_dataset: dataset,
        }
    }

    fn commissioner(default_thread: bool) -> Commissioner {
        Commissioner::new(
            config(default_thread),
            Arc::new(FabricTable::new()),
            Arc::new(AccessControl::new()),
            Arc::new(NetworkCommissioningClient::new()),
        )
    }

    /// # Upstream: src/controller/tests/TestCommissioner.cpp::TestPairDevice
    #[test]
    fn pair_device_drives_pase_then_case() {
        let c = commissioner(true);
        let payload = SetupPayload {
            version: 0,
            vendor_id: 0xFFF1,
            product_id: 0x8001,
            commissioning_flow: crate::setup_payload::CommissioningFlow::Standard,
            rendezvous_information: crate::setup_payload::RendezvousInformationFlags(
                crate::setup_payload::RendezvousInformationFlags::BLE,
            ),
            discriminator: 0xF00,
            passcode: 20_202_021,
        };
        let device_pase = PaseSession::new_responder(
            20_202_021,
            PbkdfParameters {
                iterations: 10_000,
                salt: vec![0xAB; 16],
            },
        )
        .expect("device pase");
        let paired = c.pair_device(&payload, device_pase).expect("pair");
        assert_eq!(paired.vendor_id, 0xFFF1);
        assert_eq!(paired.fabric_label, "Cave Home");
        assert_eq!(c.list_paired().len(), 1);
    }

    #[test]
    fn pair_device_then_unpair_removes_fabric() {
        let c = commissioner(false);
        let payload = SetupPayload {
            version: 0,
            vendor_id: 0x1234,
            product_id: 0x5678,
            commissioning_flow: crate::setup_payload::CommissioningFlow::Standard,
            rendezvous_information: crate::setup_payload::RendezvousInformationFlags(
                crate::setup_payload::RendezvousInformationFlags::BLE,
            ),
            discriminator: 0x123,
            passcode: 12_345_679,
        };
        let device_pase = PaseSession::new_responder(
            12_345_679,
            PbkdfParameters {
                iterations: 10_000,
                salt: vec![0xAB; 16],
            },
        )
        .expect("device pase");
        let paired = c.pair_device(&payload, device_pase).expect("pair");
        let removed = c.unpair_device(paired.node_id).expect("unpair");
        assert_eq!(removed.node_id, paired.node_id);
        assert_eq!(c.list_paired().len(), 0);
    }

    #[test]
    fn pair_rejects_passcode_mismatch() {
        let c = commissioner(false);
        let payload = SetupPayload {
            version: 0,
            vendor_id: 0,
            product_id: 0,
            commissioning_flow: crate::setup_payload::CommissioningFlow::Standard,
            rendezvous_information: crate::setup_payload::RendezvousInformationFlags(
                crate::setup_payload::RendezvousInformationFlags::BLE,
            ),
            discriminator: 0x10,
            passcode: 11_111_119,
        };
        let device_pase = PaseSession::new_responder(
            22_222_229,
            PbkdfParameters {
                iterations: 10_000,
                salt: vec![0xAB; 16],
            },
        )
        .expect("device pase");
        assert!(c.pair_device(&payload, device_pase).is_err());
    }

    #[test]
    fn unpair_unknown_errors() {
        let c = commissioner(false);
        assert!(c.unpair_device(NodeId(99)).is_err());
    }

    #[test]
    fn decode_setup_payload_qr_round_trip() {
        let p = SetupPayload {
            version: 0,
            vendor_id: 0xFFF1,
            product_id: 0x8001,
            commissioning_flow: crate::setup_payload::CommissioningFlow::Standard,
            rendezvous_information: crate::setup_payload::RendezvousInformationFlags(
                crate::setup_payload::RendezvousInformationFlags::BLE,
            ),
            discriminator: 0xF00,
            passcode: 20_202_021,
        };
        let qr = crate::setup_payload::encode_qr_payload(&p).expect("encode");
        let parsed = Commissioner::decode_setup_payload(&qr).expect("decode");
        assert_eq!(parsed, p);
    }
}
