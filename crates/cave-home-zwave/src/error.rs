// SPDX-License-Identifier: Apache-2.0
//! Decode/encode error type for the Z-Wave Command Class engine.
//!
//! Every parser in this crate returns a [`ZwaveResult`]; nothing ever panics on
//! malformed input. The discriminants mirror the failure modes a real radio
//! frame produces: a payload that is too short to hold the fields the command
//! requires, a value outside the range the Command Class specification allows,
//! or a Command Class / command byte the engine does not (yet) model.

/// Why a Command Class payload could not be decoded (or a value encoded).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZwaveError {
    /// The payload ended before all required fields were present.
    ///
    /// `need` is the minimum byte count the command requires; `got` is what the
    /// payload actually carried.
    Truncated {
        /// Minimum bytes the command needs.
        need: usize,
        /// Bytes actually present.
        got: usize,
    },

    /// A field carried a value the specification does not permit (e.g. a
    /// Multilevel Switch level of 100, which is reserved).
    OutOfRange {
        /// What field was bad (a short static identifier, not user-facing).
        field: &'static str,
        /// The offending value.
        value: u32,
    },

    /// The Command Class byte does not match the command being decoded.
    UnexpectedCommandClass {
        /// Command Class id the decoder expected.
        expected: u8,
        /// Command Class id the payload carried.
        got: u8,
    },

    /// The command id within the Command Class is unknown / not modelled.
    UnknownCommand {
        /// Command Class id.
        command_class: u8,
        /// Command id within that class.
        command: u8,
    },

    /// A size/precision field nominated a byte width the spec does not allow
    /// (the spec permits 1, 2 or 4 octets for fixed-point values).
    BadValueSize {
        /// The illegal size in octets.
        size: u8,
    },
}

impl core::fmt::Display for ZwaveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated { need, got } => {
                write!(f, "payload truncated: need {need} bytes, got {got}")
            }
            Self::OutOfRange { field, value } => {
                write!(f, "value {value} out of range for field {field}")
            }
            Self::UnexpectedCommandClass { expected, got } => {
                write!(
                    f,
                    "unexpected command class: expected {expected:#04x}, got {got:#04x}"
                )
            }
            Self::UnknownCommand {
                command_class,
                command,
            } => write!(
                f,
                "unknown command {command:#04x} for command class {command_class:#04x}"
            ),
            Self::BadValueSize { size } => {
                write!(f, "illegal fixed-point value size: {size} octets")
            }
        }
    }
}

impl std::error::Error for ZwaveError {}

/// Crate-wide `Result` alias.
pub type ZwaveResult<T> = Result<T, ZwaveError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_descriptive() {
        let e = ZwaveError::Truncated { need: 4, got: 1 };
        assert!(e.to_string().contains("truncated"));
        let e = ZwaveError::BadValueSize { size: 3 };
        assert!(e.to_string().contains('3'));
    }
}
