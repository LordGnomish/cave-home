// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.2 (CSA public PDF) only; Z2M source NOT consulted
//! Color Control cluster (0x0300) — ZCL §3.2.
//!
//! Colour and tunable-white bulbs. The persona drags a colour wheel or a
//! warm/cool slider ("Salon Lambası — turuncu", "yatak odası — sıcak
//! beyaz"); under the hood that is a Move-to-Color / Move-to-Color-Temperature
//! command and the `CurrentHue`/`CurrentX`/`ColorTemperatureMireds` attributes.
//!
//! Phase 1 implements the hue/saturation, CIE xy, and colour-temperature
//! command families (§3.2.11) plus the attribute state (§3.2.2.2) and the
//! `ColorMode` that records which colour space is currently authoritative.

use crate::error::{Result, ZigbeeError};

/// Color Control cluster identifier (ZCL §3.2.1).
pub const COLOR_CONTROL_CLUSTER_ID: u16 = 0x0300;

/// Maximum valid hue / saturation value (0xff reserved per §3.2.2.2).
pub const MAX_COMPONENT: u8 = 0xfe;

/// Received-command identifiers — ZCL §3.2.11.
pub mod command_id {
    /// Move to Hue (0x00).
    pub const MOVE_TO_HUE: u8 = 0x00;
    /// Move Hue (0x01).
    pub const MOVE_HUE: u8 = 0x01;
    /// Step Hue (0x02).
    pub const STEP_HUE: u8 = 0x02;
    /// Move to Saturation (0x03).
    pub const MOVE_TO_SATURATION: u8 = 0x03;
    /// Move Saturation (0x04).
    pub const MOVE_SATURATION: u8 = 0x04;
    /// Step Saturation (0x05).
    pub const STEP_SATURATION: u8 = 0x05;
    /// Move to Hue and Saturation (0x06).
    pub const MOVE_TO_HUE_AND_SATURATION: u8 = 0x06;
    /// Move to Color (0x07).
    pub const MOVE_TO_COLOR: u8 = 0x07;
    /// Move to Color Temperature (0x0a).
    pub const MOVE_TO_COLOR_TEMPERATURE: u8 = 0x0a;
}

/// Attribute identifiers — ZCL §3.2.2.2.
pub mod attribute_id {
    /// `CurrentHue` (0x0000, uint8).
    pub const CURRENT_HUE: u16 = 0x0000;
    /// `CurrentSaturation` (0x0001, uint8).
    pub const CURRENT_SATURATION: u16 = 0x0001;
    /// `CurrentX` (0x0003, uint16).
    pub const CURRENT_X: u16 = 0x0003;
    /// `CurrentY` (0x0004, uint16).
    pub const CURRENT_Y: u16 = 0x0004;
    /// `ColorTemperatureMireds` (0x0007, uint16).
    pub const COLOR_TEMPERATURE_MIREDS: u16 = 0x0007;
    /// `ColorMode` (0x0008, enum8).
    pub const COLOR_MODE: u16 = 0x0008;
    /// `ColorCapabilities` (0x400a, bitmap16).
    pub const COLOR_CAPABILITIES: u16 = 0x400a;
    /// `ColorTempPhysicalMinMireds` (0x400b, uint16).
    pub const COLOR_TEMP_PHYSICAL_MIN_MIREDS: u16 = 0x400b;
    /// `ColorTempPhysicalMaxMireds` (0x400c, uint16).
    pub const COLOR_TEMP_PHYSICAL_MAX_MIREDS: u16 = 0x400c;
}

/// `ColorMode` attribute (§3.2.2.2.8) — which colour space is authoritative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMode {
    /// 0x00 — `CurrentHue` and `CurrentSaturation`.
    CurrentHueAndSaturation,
    /// 0x01 — `CurrentX` and `CurrentY` (CIE xy).
    CurrentXy,
    /// 0x02 — `ColorTemperatureMireds`.
    ColorTemperatureMireds,
}

impl ColorMode {
    /// Decode from the wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::CurrentHueAndSaturation),
            0x01 => Ok(Self::CurrentXy),
            0x02 => Ok(Self::ColorTemperatureMireds),
            other => Err(ZigbeeError::Zcl(format!("reserved color mode 0x{other:02x}"))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::CurrentHueAndSaturation => 0x00,
            Self::CurrentXy => 0x01,
            Self::ColorTemperatureMireds => 0x02,
        }
    }
}

/// Hue movement direction — ZCL §3.2.11.2 `direction` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HueDirection {
    /// 0x00 — shortest distance.
    ShortestDistance,
    /// 0x01 — longest distance.
    LongestDistance,
    /// 0x02 — up.
    Up,
    /// 0x03 — down.
    Down,
}

impl HueDirection {
    /// Decode from the wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::ShortestDistance),
            0x01 => Ok(Self::LongestDistance),
            0x02 => Ok(Self::Up),
            0x03 => Ok(Self::Down),
            other => Err(ZigbeeError::Zcl(format!("reserved hue direction 0x{other:02x}"))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::ShortestDistance => 0x00,
            Self::LongestDistance => 0x01,
            Self::Up => 0x02,
            Self::Down => 0x03,
        }
    }
}

/// Continuous-move mode — ZCL §3.2.11.3 (Move Hue / Move Saturation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMoveMode {
    /// 0x00 — stop any in-progress move.
    Stop,
    /// 0x01 — move up.
    Up,
    /// 0x03 — move down.
    Down,
}

impl ColorMoveMode {
    /// Decode from the wire value (0x02 is reserved).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::Stop),
            0x01 => Ok(Self::Up),
            0x03 => Ok(Self::Down),
            other => Err(ZigbeeError::Zcl(format!("reserved color move mode 0x{other:02x}"))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Stop => 0x00,
            Self::Up => 0x01,
            Self::Down => 0x03,
        }
    }
}

/// Step mode — ZCL §3.2.11.4 (Step Hue / Step Saturation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorStepMode {
    /// 0x01 — step up.
    Up,
    /// 0x03 — step down.
    Down,
}

impl ColorStepMode {
    /// Decode from the wire value (0x00 and 0x02 are reserved).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x01 => Ok(Self::Up),
            0x03 => Ok(Self::Down),
            other => Err(ZigbeeError::Zcl(format!("reserved color step mode 0x{other:02x}"))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Up => 0x01,
            Self::Down => 0x03,
        }
    }
}

/// A decoded Color Control cluster-specific command (client → server).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorCommand {
    /// Move to Hue (0x00).
    MoveToHue {
        /// Target hue (0..=254).
        hue: u8,
        /// Direction around the colour wheel.
        direction: HueDirection,
        /// Transition time (1/10 s units).
        transition_time: u16,
    },
    /// Move Hue (0x01) — continuous.
    MoveHue {
        /// Move mode.
        mode: ColorMoveMode,
        /// Rate (units/second).
        rate: u8,
    },
    /// Step Hue (0x02).
    StepHue {
        /// Step direction.
        mode: ColorStepMode,
        /// Step magnitude.
        step_size: u8,
        /// Transition time (1/10 s units, 8-bit per §3.2.11.5).
        transition_time: u8,
    },
    /// Move to Saturation (0x03).
    MoveToSaturation {
        /// Target saturation (0..=254).
        saturation: u8,
        /// Transition time (1/10 s units).
        transition_time: u16,
    },
    /// Move Saturation (0x04) — continuous.
    MoveSaturation {
        /// Move mode.
        mode: ColorMoveMode,
        /// Rate (units/second).
        rate: u8,
    },
    /// Step Saturation (0x05).
    StepSaturation {
        /// Step direction.
        mode: ColorStepMode,
        /// Step magnitude.
        step_size: u8,
        /// Transition time (1/10 s units, 8-bit per §3.2.11.6).
        transition_time: u8,
    },
    /// Move to Hue and Saturation (0x06).
    MoveToHueAndSaturation {
        /// Target hue (0..=254).
        hue: u8,
        /// Target saturation (0..=254).
        saturation: u8,
        /// Transition time (1/10 s units).
        transition_time: u16,
    },
    /// Move to Color (0x07) — CIE xy.
    MoveToColor {
        /// Target CIE x (uint16).
        color_x: u16,
        /// Target CIE y (uint16).
        color_y: u16,
        /// Transition time (1/10 s units).
        transition_time: u16,
    },
    /// Move to Color Temperature (0x0a).
    MoveToColorTemperature {
        /// Target colour temperature in mireds (uint16).
        color_temp_mireds: u16,
        /// Transition time (1/10 s units).
        transition_time: u16,
    },
}

impl ColorCommand {
    /// Parse a cluster-specific command from its id + raw payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] for short payloads, [`ZigbeeError::Zcl`]
    /// for unknown command ids or reserved mode/direction fields.
    pub fn parse(command_id: u8, payload: &[u8]) -> Result<Self> {
        match command_id {
            command_id::MOVE_TO_HUE => {
                require(payload, 4)?;
                Ok(Self::MoveToHue {
                    hue: payload[0],
                    direction: HueDirection::from_u8(payload[1])?,
                    transition_time: u16::from_le_bytes([payload[2], payload[3]]),
                })
            }
            command_id::MOVE_HUE => {
                require(payload, 2)?;
                Ok(Self::MoveHue {
                    mode: ColorMoveMode::from_u8(payload[0])?,
                    rate: payload[1],
                })
            }
            command_id::STEP_HUE => {
                require(payload, 3)?;
                Ok(Self::StepHue {
                    mode: ColorStepMode::from_u8(payload[0])?,
                    step_size: payload[1],
                    transition_time: payload[2],
                })
            }
            command_id::MOVE_TO_SATURATION => {
                require(payload, 3)?;
                Ok(Self::MoveToSaturation {
                    saturation: payload[0],
                    transition_time: u16::from_le_bytes([payload[1], payload[2]]),
                })
            }
            command_id::MOVE_SATURATION => {
                require(payload, 2)?;
                Ok(Self::MoveSaturation {
                    mode: ColorMoveMode::from_u8(payload[0])?,
                    rate: payload[1],
                })
            }
            command_id::STEP_SATURATION => {
                require(payload, 3)?;
                Ok(Self::StepSaturation {
                    mode: ColorStepMode::from_u8(payload[0])?,
                    step_size: payload[1],
                    transition_time: payload[2],
                })
            }
            command_id::MOVE_TO_HUE_AND_SATURATION => {
                require(payload, 4)?;
                Ok(Self::MoveToHueAndSaturation {
                    hue: payload[0],
                    saturation: payload[1],
                    transition_time: u16::from_le_bytes([payload[2], payload[3]]),
                })
            }
            command_id::MOVE_TO_COLOR => {
                require(payload, 6)?;
                Ok(Self::MoveToColor {
                    color_x: u16::from_le_bytes([payload[0], payload[1]]),
                    color_y: u16::from_le_bytes([payload[2], payload[3]]),
                    transition_time: u16::from_le_bytes([payload[4], payload[5]]),
                })
            }
            command_id::MOVE_TO_COLOR_TEMPERATURE => {
                require(payload, 4)?;
                Ok(Self::MoveToColorTemperature {
                    color_temp_mireds: u16::from_le_bytes([payload[0], payload[1]]),
                    transition_time: u16::from_le_bytes([payload[2], payload[3]]),
                })
            }
            other => Err(ZigbeeError::Zcl(format!(
                "unknown Color Control command 0x{other:02x}"
            ))),
        }
    }

    /// The command identifier for this command.
    #[must_use]
    pub const fn command_id(&self) -> u8 {
        match self {
            Self::MoveToHue { .. } => command_id::MOVE_TO_HUE,
            Self::MoveHue { .. } => command_id::MOVE_HUE,
            Self::StepHue { .. } => command_id::STEP_HUE,
            Self::MoveToSaturation { .. } => command_id::MOVE_TO_SATURATION,
            Self::MoveSaturation { .. } => command_id::MOVE_SATURATION,
            Self::StepSaturation { .. } => command_id::STEP_SATURATION,
            Self::MoveToHueAndSaturation { .. } => command_id::MOVE_TO_HUE_AND_SATURATION,
            Self::MoveToColor { .. } => command_id::MOVE_TO_COLOR,
            Self::MoveToColorTemperature { .. } => command_id::MOVE_TO_COLOR_TEMPERATURE,
        }
    }

    /// Encode the command-specific payload (header excluded).
    #[must_use]
    pub fn encode_payload(&self) -> Vec<u8> {
        match self {
            Self::MoveToHue {
                hue,
                direction,
                transition_time,
            } => {
                let mut out = Vec::with_capacity(4);
                out.push(*hue);
                out.push(direction.to_u8());
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
            Self::MoveHue { mode, rate } | Self::MoveSaturation { mode, rate } => {
                vec![mode.to_u8(), *rate]
            }
            Self::StepHue {
                mode,
                step_size,
                transition_time,
            }
            | Self::StepSaturation {
                mode,
                step_size,
                transition_time,
            } => vec![mode.to_u8(), *step_size, *transition_time],
            Self::MoveToSaturation {
                saturation,
                transition_time,
            } => {
                let mut out = Vec::with_capacity(3);
                out.push(*saturation);
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
            Self::MoveToHueAndSaturation {
                hue,
                saturation,
                transition_time,
            } => {
                let mut out = Vec::with_capacity(4);
                out.push(*hue);
                out.push(*saturation);
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
            Self::MoveToColor {
                color_x,
                color_y,
                transition_time,
            } => {
                let mut out = Vec::with_capacity(6);
                out.extend_from_slice(&color_x.to_le_bytes());
                out.extend_from_slice(&color_y.to_le_bytes());
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
            Self::MoveToColorTemperature {
                color_temp_mireds,
                transition_time,
            } => {
                let mut out = Vec::with_capacity(4);
                out.extend_from_slice(&color_temp_mireds.to_le_bytes());
                out.extend_from_slice(&transition_time.to_le_bytes());
                out
            }
        }
    }
}

/// In-memory Color Control state for one endpoint (§3.2.2.2 attributes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorControlState {
    /// `CurrentHue` attribute (0x0000).
    pub current_hue: u8,
    /// `CurrentSaturation` attribute (0x0001).
    pub current_saturation: u8,
    /// `CurrentX` attribute (0x0003).
    pub current_x: u16,
    /// `CurrentY` attribute (0x0004).
    pub current_y: u16,
    /// `ColorTemperatureMireds` attribute (0x0007).
    pub color_temperature_mireds: u16,
    /// `ColorMode` attribute (0x0008).
    pub color_mode: ColorMode,
}

impl Default for ColorControlState {
    fn default() -> Self {
        Self {
            current_hue: 0,
            current_saturation: 0,
            // §3.2.2.2.4/5: default CurrentX/Y ≈ 0.4607/0.4151 (warm white).
            current_x: 0x616b,
            current_y: 0x607d,
            // §3.2.2.2.9: default ColorTemperatureMireds is 0x00fa (4000 K).
            color_temperature_mireds: 0x00fa,
            // §3.2.2.2.8: default ColorMode is CurrentXy.
            color_mode: ColorMode::CurrentXy,
        }
    }
}

impl ColorControlState {
    /// Fresh state — warm white, defaults per §3.2.2.2.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a received command, mutating the attribute state per §3.2.11.
    /// Transitions are modelled as instantaneous (the target value); the
    /// time-domain ramp is the device's job. Setting hue/saturation, xy, or
    /// colour temperature also switches `ColorMode` to the matching space.
    pub fn apply(&mut self, cmd: &ColorCommand) {
        match cmd {
            ColorCommand::MoveToHue { hue, .. } => {
                self.current_hue = (*hue).min(MAX_COMPONENT);
                self.color_mode = ColorMode::CurrentHueAndSaturation;
            }
            ColorCommand::StepHue {
                mode, step_size, ..
            } => {
                self.current_hue = step_hue(self.current_hue, *mode, *step_size);
                self.color_mode = ColorMode::CurrentHueAndSaturation;
            }
            ColorCommand::MoveToSaturation { saturation, .. } => {
                self.current_saturation = (*saturation).min(MAX_COMPONENT);
                self.color_mode = ColorMode::CurrentHueAndSaturation;
            }
            ColorCommand::StepSaturation {
                mode, step_size, ..
            } => {
                self.current_saturation = match mode {
                    ColorStepMode::Up => {
                        self.current_saturation.saturating_add(*step_size).min(MAX_COMPONENT)
                    }
                    ColorStepMode::Down => self.current_saturation.saturating_sub(*step_size),
                };
                self.color_mode = ColorMode::CurrentHueAndSaturation;
            }
            ColorCommand::MoveToHueAndSaturation {
                hue, saturation, ..
            } => {
                self.current_hue = (*hue).min(MAX_COMPONENT);
                self.current_saturation = (*saturation).min(MAX_COMPONENT);
                self.color_mode = ColorMode::CurrentHueAndSaturation;
            }
            ColorCommand::MoveToColor {
                color_x, color_y, ..
            } => {
                self.current_x = *color_x;
                self.current_y = *color_y;
                self.color_mode = ColorMode::CurrentXy;
            }
            ColorCommand::MoveToColorTemperature {
                color_temp_mireds, ..
            } => {
                self.color_temperature_mireds = *color_temp_mireds;
                self.color_mode = ColorMode::ColorTemperatureMireds;
            }
            // Continuous moves only switch the colour space; the latched
            // value tracks during the ramp on the device and is reported back.
            ColorCommand::MoveHue { .. } | ColorCommand::MoveSaturation { .. } => {
                self.color_mode = ColorMode::CurrentHueAndSaturation;
            }
        }
    }
}

/// Step the circular hue (0..=254, 0xff reserved) — wraps modulo 255.
const fn step_hue(hue: u8, mode: ColorStepMode, step: u8) -> u8 {
    // Hue space has 255 valid positions (0..=254); arithmetic is modulo 255.
    let span: u16 = MAX_COMPONENT as u16 + 1; // 255
    let cur = hue as u16 % span;
    let delta = step as u16 % span;
    let next = match mode {
        ColorStepMode::Up => (cur + delta) % span,
        ColorStepMode::Down => (cur + span - delta) % span,
    };
    // `next` is `_ % 255`, so it is always in 0..=254 and fits in u8.
    #[allow(clippy::cast_possible_truncation)]
    let hue = next as u8;
    hue
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
