// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.10 (CSA public PDF) only; Z2M source NOT consulted
//! Level Control cluster (0x0008) — ZCL §3.10.
//!
//! Dimming. The headline persona drags a brightness slider ("Salon
//! Lambası — %50"); under the hood that is a Move-to-Level command and a
//! `CurrentLevel` attribute on the 0..=254 scale.
//!
//! Phase 1 implements the full received-command set (§3.10.2.4) and the
//! attribute state (§3.10.2.3) needed to track brightness, including the
//! `StartUpCurrentLevel` power-on behaviour and the `OnOff` coupling of the
//! `*WithOnOff` command variants.

use crate::error::{Result, ZigbeeError};

/// Level Control cluster identifier (ZCL §3.10.1).
pub const LEVEL_CONTROL_CLUSTER_ID: u16 = 0x0008;

/// Maximum valid level (0xff is reserved per §3.10.2.3.1).
pub const MAX_LEVEL: u8 = 0xfe;

/// Received-command identifiers — ZCL §3.10.2.4.
pub mod command_id {
    /// Move to Level (0x00).
    pub const MOVE_TO_LEVEL: u8 = 0x00;
    /// Move (0x01).
    pub const MOVE: u8 = 0x01;
    /// Step (0x02).
    pub const STEP: u8 = 0x02;
    /// Stop (0x03).
    pub const STOP: u8 = 0x03;
    /// Move to Level (with On/Off) (0x04).
    pub const MOVE_TO_LEVEL_WITH_ON_OFF: u8 = 0x04;
    /// Move (with On/Off) (0x05).
    pub const MOVE_WITH_ON_OFF: u8 = 0x05;
    /// Step (with On/Off) (0x06).
    pub const STEP_WITH_ON_OFF: u8 = 0x06;
    /// Stop (with On/Off) (0x07).
    pub const STOP_WITH_ON_OFF: u8 = 0x07;
}

/// Attribute identifiers — ZCL §3.10.2.3.
pub mod attribute_id {
    /// `CurrentLevel` (0x0000, uint8).
    pub const CURRENT_LEVEL: u16 = 0x0000;
    /// `RemainingTime` (0x0001, uint16, 1/10 s units).
    pub const REMAINING_TIME: u16 = 0x0001;
    /// Options (0x000f, bitmap8).
    pub const OPTIONS: u16 = 0x000f;
    /// `OnLevel` (0x0011, uint8).
    pub const ON_LEVEL: u16 = 0x0011;
    /// `StartUpCurrentLevel` (0x4000, uint8).
    pub const START_UP_CURRENT_LEVEL: u16 = 0x4000;
}

/// Move direction — ZCL §3.10.2.4.2 `move_mode` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveMode {
    /// 0x00 — increase the level.
    Up,
    /// 0x01 — decrease the level.
    Down,
}

impl MoveMode {
    /// Decode from the wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::Up),
            0x01 => Ok(Self::Down),
            other => Err(ZigbeeError::Zcl(format!("reserved move mode 0x{other:02x}"))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Up => 0x00,
            Self::Down => 0x01,
        }
    }
}

/// Step direction — ZCL §3.10.2.4.3 `step_mode` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepMode {
    /// 0x00 — increase the level by `step_size`.
    Up,
    /// 0x01 — decrease the level by `step_size`.
    Down,
}

impl StepMode {
    /// Decode from the wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::Up),
            0x01 => Ok(Self::Down),
            other => Err(ZigbeeError::Zcl(format!("reserved step mode 0x{other:02x}"))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Up => 0x00,
            Self::Down => 0x01,
        }
    }
}

/// A decoded Level Control cluster-specific command (client → server).
///
/// The four `*WithOnOff` variants share their payload with their plain
/// counterparts but also drive the `OnOff` cluster — modelled here by the
/// `with_on_off` flag rather than four extra enum arms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelCommand {
    /// Move to Level (0x00 / 0x04).
    MoveToLevel {
        /// Target level (0..=254).
        level: u8,
        /// Transition time in 1/10 s units (0xffff = use device default).
        transition_time: u16,
        /// `true` for the 0x04 with-On/Off variant.
        with_on_off: bool,
    },
    /// Move (0x01 / 0x05).
    Move {
        /// Direction.
        mode: MoveMode,
        /// Rate in units/second (0xff = as fast as possible).
        rate: u8,
        /// `true` for the 0x05 with-On/Off variant.
        with_on_off: bool,
    },
    /// Step (0x02 / 0x06).
    Step {
        /// Direction.
        mode: StepMode,
        /// Step magnitude.
        step_size: u8,
        /// Transition time in 1/10 s units.
        transition_time: u16,
        /// `true` for the 0x06 with-On/Off variant.
        with_on_off: bool,
    },
    /// Stop (0x03 / 0x07) — halt any in-progress move/step.
    Stop {
        /// `true` for the 0x07 with-On/Off variant.
        with_on_off: bool,
    },
}

impl LevelCommand {
    /// Parse a cluster-specific command from its id + raw payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] for short payloads, [`ZigbeeError::Zcl`]
    /// for unknown command ids or reserved mode fields.
    pub fn parse(command_id: u8, payload: &[u8]) -> Result<Self> {
        match command_id {
            command_id::MOVE_TO_LEVEL | command_id::MOVE_TO_LEVEL_WITH_ON_OFF => {
                require(payload, 3)?;
                Ok(Self::MoveToLevel {
                    level: payload[0],
                    transition_time: u16::from_le_bytes([payload[1], payload[2]]),
                    with_on_off: command_id == command_id::MOVE_TO_LEVEL_WITH_ON_OFF,
                })
            }
            command_id::MOVE | command_id::MOVE_WITH_ON_OFF => {
                require(payload, 2)?;
                Ok(Self::Move {
                    mode: MoveMode::from_u8(payload[0])?,
                    rate: payload[1],
                    with_on_off: command_id == command_id::MOVE_WITH_ON_OFF,
                })
            }
            command_id::STEP | command_id::STEP_WITH_ON_OFF => {
                require(payload, 4)?;
                Ok(Self::Step {
                    mode: StepMode::from_u8(payload[0])?,
                    step_size: payload[1],
                    transition_time: u16::from_le_bytes([payload[2], payload[3]]),
                    with_on_off: command_id == command_id::STEP_WITH_ON_OFF,
                })
            }
            command_id::STOP | command_id::STOP_WITH_ON_OFF => Ok(Self::Stop {
                with_on_off: command_id == command_id::STOP_WITH_ON_OFF,
            }),
            other => Err(ZigbeeError::Zcl(format!(
                "unknown Level Control command 0x{other:02x}"
            ))),
        }
    }

    /// The command identifier for this command (selects the with-On/Off id
    /// when `with_on_off` is set).
    #[must_use]
    pub const fn command_id(&self) -> u8 {
        match self {
            Self::MoveToLevel { with_on_off, .. } => {
                if *with_on_off {
                    command_id::MOVE_TO_LEVEL_WITH_ON_OFF
                } else {
                    command_id::MOVE_TO_LEVEL
                }
            }
            Self::Move { with_on_off, .. } => {
                if *with_on_off {
                    command_id::MOVE_WITH_ON_OFF
                } else {
                    command_id::MOVE
                }
            }
            Self::Step { with_on_off, .. } => {
                if *with_on_off {
                    command_id::STEP_WITH_ON_OFF
                } else {
                    command_id::STEP
                }
            }
            Self::Stop { with_on_off } => {
                if *with_on_off {
                    command_id::STOP_WITH_ON_OFF
                } else {
                    command_id::STOP
                }
            }
        }
    }

    /// Encode the command-specific payload (header excluded).
    #[must_use]
    pub fn encode_payload(&self) -> Vec<u8> {
        match self {
            Self::MoveToLevel {
                level,
                transition_time,
                ..
            } => {
                let mut out = Vec::with_capacity(3);
                out.push(*level);
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
            Self::Move { mode, rate, .. } => vec![mode.to_u8(), *rate],
            Self::Step {
                mode,
                step_size,
                transition_time,
                ..
            } => {
                let mut out = Vec::with_capacity(4);
                out.push(mode.to_u8());
                out.push(*step_size);
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
            Self::Stop { .. } => Vec::new(),
        }
    }

    /// If this is a `*WithOnOff` command, the `OnOff` state it implies:
    /// `Some(true)` ⇒ turn on (target level > 0), `Some(false)` ⇒ turn off
    /// (target level 0). Plain (non-On/Off) commands return `None`.
    ///
    /// Per §3.10.2.4: Move-up / Step-up / Move-to a nonzero level turn the
    /// device on; reaching level 0 turns it off.
    #[must_use]
    pub const fn couples_on_off(&self) -> Option<bool> {
        match self {
            Self::MoveToLevel {
                level,
                with_on_off: true,
                ..
            } => Some(*level > 0),
            Self::Move {
                mode: MoveMode::Up,
                with_on_off: true,
                ..
            }
            | Self::Step {
                mode: StepMode::Up,
                with_on_off: true,
                ..
            } => Some(true),
            Self::Move {
                mode: MoveMode::Down,
                with_on_off: true,
                ..
            }
            | Self::Step {
                mode: StepMode::Down,
                with_on_off: true,
                ..
            } => Some(false),
            _ => None,
        }
    }
}

/// `StartUpCurrentLevel` attribute (§3.10.2.3.13) — power-on level behaviour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartUpCurrentLevel {
    /// 0x00 — set `CurrentLevel` to the minimum (1) on power-up.
    Minimum,
    /// 0xff — keep the previous `CurrentLevel` (default).
    Previous,
    /// Any other value — set `CurrentLevel` to exactly that value.
    Level(u8),
}

impl StartUpCurrentLevel {
    /// Decode from the wire value. (Total — no reserved values.)
    #[must_use]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0x00 => Self::Minimum,
            0xff => Self::Previous,
            other => Self::Level(other),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Minimum => 0x00,
            Self::Previous => 0xff,
            Self::Level(v) => v,
        }
    }
}

/// In-memory Level Control state for one endpoint (§3.10.2.3 attributes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LevelControlState {
    /// `CurrentLevel` attribute (0x0000).
    pub current_level: u8,
    /// `RemainingTime` attribute (0x0001).
    pub remaining_time: u16,
    /// `OnLevel` attribute (0x0011) — level used when turned on via `OnOff`.
    pub on_level: Option<u8>,
    /// `StartUpCurrentLevel` attribute (0x4000).
    pub start_up_current_level: StartUpCurrentLevel,
}

impl Default for LevelControlState {
    fn default() -> Self {
        Self {
            // §3.10.2.3.1: power-up default for CurrentLevel is 0xfe.
            current_level: MAX_LEVEL,
            remaining_time: 0,
            on_level: None,
            start_up_current_level: StartUpCurrentLevel::Previous,
        }
    }
}

impl LevelControlState {
    /// Fresh state — full brightness, defaults per §3.10.2.3.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a received command to `current_level`. Transitions are modelled
    /// as instantaneous (the target level); the time-domain ramp is the
    /// device's job. Move/Step clamp to the 0..=254 range.
    pub fn apply(&mut self, cmd: &LevelCommand) {
        match cmd {
            LevelCommand::MoveToLevel { level, .. } => {
                self.current_level = (*level).min(MAX_LEVEL);
            }
            LevelCommand::Move { mode, .. } => {
                // No Stop ⇒ the move runs to the relevant limit.
                self.current_level = match mode {
                    MoveMode::Up => MAX_LEVEL,
                    MoveMode::Down => 0,
                };
            }
            LevelCommand::Step {
                mode, step_size, ..
            } => {
                self.current_level = match mode {
                    StepMode::Up => self.current_level.saturating_add(*step_size).min(MAX_LEVEL),
                    StepMode::Down => self.current_level.saturating_sub(*step_size),
                };
            }
            LevelCommand::Stop { .. } => {
                // Halts an in-progress ramp; the latched level is unchanged.
                self.remaining_time = 0;
            }
        }
    }

    /// Apply the `StartUpCurrentLevel` power-on rule (§3.10.2.3.13). Call on
    /// (re)boot with `current_level` holding the persisted value.
    pub fn power_on(&mut self) {
        self.current_level = match self.start_up_current_level {
            StartUpCurrentLevel::Minimum => 1,
            StartUpCurrentLevel::Previous => self.current_level,
            StartUpCurrentLevel::Level(v) => v.min(MAX_LEVEL),
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
