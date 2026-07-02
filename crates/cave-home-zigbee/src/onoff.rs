// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.8 (CSA public PDF) only; Z2M source NOT consulted
//! `OnOff` cluster (0x0006) — ZCL §3.8.
//!
//! The bread-and-butter cluster: every switch, plug, and bulb exposes it.
//! The headline persona just sees a toggle in the Portal ("Salon Lambası"
//! Aç / Kapat) — they never see the cluster id or the command bytes.
//!
//! Phase 1 implements the full received-command set (§3.8.2.3) and the
//! state attributes (§3.8.2.2) needed to track and report a device's
//! on/off state, including the `StartUpOnOff` power-on behaviour that
//! decides whether a bulb comes back lit after a power cut.

use crate::error::{Result, ZigbeeError};

/// `OnOff` cluster identifier (ZCL §3.8.1).
pub const ON_OFF_CLUSTER_ID: u16 = 0x0006;

/// Received-command identifiers — ZCL §3.8.2.3.
pub mod command_id {
    /// Off (0x00) — turn the device off.
    pub const OFF: u8 = 0x00;
    /// On (0x01) — turn the device on.
    pub const ON: u8 = 0x01;
    /// Toggle (0x02) — invert the current state.
    pub const TOGGLE: u8 = 0x02;
    /// Off with effect (0x40) — fade/effect then off.
    pub const OFF_WITH_EFFECT: u8 = 0x40;
    /// On with recall global scene (0x41).
    pub const ON_WITH_RECALL_GLOBAL_SCENE: u8 = 0x41;
    /// On with timed off (0x42) — on, then auto-off after a timer.
    pub const ON_WITH_TIMED_OFF: u8 = 0x42;
}

/// Attribute identifiers — ZCL §3.8.2.2.
pub mod attribute_id {
    /// `OnOff` (0x0000, bool) — the current on/off state.
    pub const ON_OFF: u16 = 0x0000;
    /// `GlobalSceneControl` (0x4000, bool).
    pub const GLOBAL_SCENE_CONTROL: u16 = 0x4000;
    /// `OnTime` (0x4001, uint16, 1/10 s units).
    pub const ON_TIME: u16 = 0x4001;
    /// `OffWaitTime` (0x4002, uint16, 1/10 s units).
    pub const OFF_WAIT_TIME: u16 = 0x4002;
    /// `StartUpOnOff` (0x4003, enum8).
    pub const START_UP_ON_OFF: u16 = 0x4003;
}

/// A decoded `OnOff` cluster-specific command (client → server).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnOffCommand {
    /// Off (0x00).
    Off,
    /// On (0x01).
    On,
    /// Toggle (0x02).
    Toggle,
    /// Off with effect (0x40) — §3.8.2.3.4.
    OffWithEffect {
        /// Effect identifier (0x00 = fade, 0x01 = dying-light, …).
        effect_id: u8,
        /// Effect variant.
        effect_variant: u8,
    },
    /// On with recall global scene (0x41).
    OnWithRecallGlobalScene,
    /// On with timed off (0x42) — §3.8.2.3.6.
    OnWithTimedOff {
        /// `on_off_control` — bit 0 = accept only when already on.
        on_off_control: u8,
        /// On time in 1/10 s units.
        on_time: u16,
        /// Off-wait time in 1/10 s units.
        off_wait_time: u16,
    },
}

impl OnOffCommand {
    /// Parse a cluster-specific command from its id + raw payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if the payload is shorter than the
    /// command requires, or [`ZigbeeError::Zcl`] for an unknown command id.
    pub fn parse(command_id: u8, payload: &[u8]) -> Result<Self> {
        match command_id {
            command_id::OFF => Ok(Self::Off),
            command_id::ON => Ok(Self::On),
            command_id::TOGGLE => Ok(Self::Toggle),
            command_id::ON_WITH_RECALL_GLOBAL_SCENE => Ok(Self::OnWithRecallGlobalScene),
            command_id::OFF_WITH_EFFECT => {
                require(payload, 2)?;
                Ok(Self::OffWithEffect {
                    effect_id: payload[0],
                    effect_variant: payload[1],
                })
            }
            command_id::ON_WITH_TIMED_OFF => {
                require(payload, 5)?;
                Ok(Self::OnWithTimedOff {
                    on_off_control: payload[0],
                    on_time: u16::from_le_bytes([payload[1], payload[2]]),
                    off_wait_time: u16::from_le_bytes([payload[3], payload[4]]),
                })
            }
            other => Err(ZigbeeError::Zcl(format!(
                "unknown OnOff command 0x{other:02x}"
            ))),
        }
    }

    /// The command identifier for this command.
    #[must_use]
    pub const fn command_id(&self) -> u8 {
        match self {
            Self::Off => command_id::OFF,
            Self::On => command_id::ON,
            Self::Toggle => command_id::TOGGLE,
            Self::OffWithEffect { .. } => command_id::OFF_WITH_EFFECT,
            Self::OnWithRecallGlobalScene => command_id::ON_WITH_RECALL_GLOBAL_SCENE,
            Self::OnWithTimedOff { .. } => command_id::ON_WITH_TIMED_OFF,
        }
    }

    /// Encode the command-specific payload (header excluded — see [`crate::zcl`]).
    #[must_use]
    pub fn encode_payload(&self) -> Vec<u8> {
        match self {
            Self::Off | Self::On | Self::Toggle | Self::OnWithRecallGlobalScene => Vec::new(),
            Self::OffWithEffect {
                effect_id,
                effect_variant,
            } => vec![*effect_id, *effect_variant],
            Self::OnWithTimedOff {
                on_off_control,
                on_time,
                off_wait_time,
            } => {
                let mut out = Vec::with_capacity(5);
                out.push(*on_off_control);
                out.extend_from_slice(&on_time.to_le_bytes());
                out.extend_from_slice(&off_wait_time.to_le_bytes());
                out
            }
        }
    }
}

/// `StartUpOnOff` attribute (§3.8.2.2.5) — power-on behaviour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartUpOnOff {
    /// 0x00 — set `OnOff` to off on power-up.
    Off,
    /// 0x01 — set `OnOff` to on on power-up.
    On,
    /// 0x02 — toggle the previous `OnOff` value on power-up.
    Toggle,
    /// 0xff — keep the previous `OnOff` value (default).
    Previous,
}

impl StartUpOnOff {
    /// Decode from the enum8 wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value (0x03..=0xfe).
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::Off),
            0x01 => Ok(Self::On),
            0x02 => Ok(Self::Toggle),
            0xff => Ok(Self::Previous),
            other => Err(ZigbeeError::Zcl(format!(
                "reserved StartUpOnOff value 0x{other:02x}"
            ))),
        }
    }

    /// Encode to the enum8 wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::On => 0x01,
            Self::Toggle => 0x02,
            Self::Previous => 0xff,
        }
    }
}

/// In-memory `OnOff` cluster state for one endpoint (§3.8.2.2 attributes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OnOffState {
    /// `OnOff` attribute (0x0000).
    pub on: bool,
    /// `GlobalSceneControl` attribute (0x4000).
    pub global_scene_control: bool,
    /// `OnTime` attribute (0x4001).
    pub on_time: u16,
    /// `OffWaitTime` attribute (0x4002).
    pub off_wait_time: u16,
    /// `StartUpOnOff` attribute (0x4003).
    pub start_up: StartUpOnOff,
}

impl Default for OnOffState {
    fn default() -> Self {
        Self {
            on: false,
            global_scene_control: true,
            on_time: 0,
            off_wait_time: 0,
            // §3.8.2.2.5: default StartUpOnOff is "previous".
            start_up: StartUpOnOff::Previous,
        }
    }
}

impl OnOffState {
    /// Fresh state — off, defaults per §3.8.2.2.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a received command, mutating the attribute state per §3.8.2.3.
    pub fn apply(&mut self, cmd: &OnOffCommand) {
        match cmd {
            OnOffCommand::Off | OnOffCommand::OffWithEffect { .. } => {
                self.on = false;
                self.on_time = 0;
            }
            OnOffCommand::On | OnOffCommand::OnWithRecallGlobalScene => {
                self.on = true;
                // §3.8.2.2.2: On sets GlobalSceneControl to TRUE.
                self.global_scene_control = true;
                self.off_wait_time = 0;
            }
            OnOffCommand::Toggle => {
                self.on = !self.on;
            }
            OnOffCommand::OnWithTimedOff {
                on_off_control,
                on_time,
                off_wait_time,
            } => {
                // §3.8.2.3.6.1: bit 0 (accept-only-when-on) gates the command.
                if on_off_control & 0x01 == 0x01 && !self.on {
                    return;
                }
                self.on = true;
                self.on_time = *on_time;
                self.off_wait_time = *off_wait_time;
            }
        }
    }

    /// Apply the `StartUpOnOff` power-on rule (§3.8.2.2.5). Call this when
    /// the device (re)boots with `self.on` holding the persisted state.
    pub const fn power_on(&mut self) {
        self.on = match self.start_up {
            StartUpOnOff::Off => false,
            StartUpOnOff::On => true,
            StartUpOnOff::Toggle => !self.on,
            StartUpOnOff::Previous => self.on,
        };
    }
}

/// Require at least `n` payload bytes.
const fn require(payload: &[u8], n: usize) -> Result<()> {
    if payload.len() < n {
        Err(ZigbeeError::Truncated {
            need: n,
            have: payload.len(),
        })
    } else {
        Ok(())
    }
}
